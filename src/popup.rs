//! Popup de conquista: barra fina com troféu (cor do nível), nome e uma linha
//! embaixo. Desenhado em memória (tiny-skia + ab_glyph) e mostrado numa janela
//! X11 marcada com `GAMESCOPE_EXTERNAL_OVERLAY`, que o gamescope desenha por
//! cima do jogo. Fora do gamescope vira uma janela override-redirect comum.

use std::time::Duration;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, point};
use anyhow::{Context, Result};
use tiny_skia::{
    FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, PremultipliedColorU8, Rect, Stroke,
    Transform,
};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::{
    AtomEnum, ColormapAlloc, ConnectionExt as _, CreateGCAux, CreateWindowAux, ImageFormat,
    PropMode, VisualClass, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

use crate::config::Nivel;

pub struct Aviso {
    pub nivel: Nivel,
    pub titulo: String,
    pub linha2: String,
    /// `Some(pct)` = aviso de progresso (desenha barra).
    pub progresso: Option<u8>,
}

const DURACAO: Duration = Duration::from_secs(5);
/// Distância do popup ao topo e à borda direita, em px a 768 linhas.
const MARGEM: f32 = 8.0;
/// Largura do popup (px a 768 linhas): acompanha o texto dentro destes limites.
const LARGURA_MIN: f32 = 200.0;
const LARGURA_MAX: f32 = 340.0;

pub fn cor_nivel(n: Nivel) -> (u8, u8, u8) {
    match n {
        Nivel::Bronze => (0xC9, 0x82, 0x4A),
        Nivel::Prata => (0xC4, 0xCA, 0xD0),
        Nivel::Ouro => (0xE8, 0xB9, 0x30),
        Nivel::Platina => (0x9F, 0xC9, 0xE8),
    }
}

/// Fontes embutidas (SIL OFL 1.1, licenças em `assets/fontes/`).
pub const FONTE_TITULO: &[u8] = include_bytes!("../assets/fontes/GrenzeGotisch.ttf");
pub const FONTE_TEXTO: &[u8] = include_bytes!("../assets/fontes/TitilliumWeb-Regular.ttf");

/// Fonte do arquivo em `$var` (para testar outras) ou a embutida.
fn fonte(var: &str, embutida: &'static [u8]) -> Result<FontVec> {
    let dados = match std::env::var_os(var) {
        Some(c) => {
            std::fs::read(&c).with_context(|| format!("ler fonte {}", c.to_string_lossy()))?
        }
        None => embutida.to_vec(),
    };
    FontVec::try_from_vec(dados).with_context(|| format!("fonte inválida ({var})"))
}

struct Fontes {
    titulo: FontVec,
    texto: FontVec,
}

impl Fontes {
    fn carregar() -> Result<Self> {
        Ok(Fontes {
            titulo: fonte("SPEEDCONQUEST_FONTE_TITULO", FONTE_TITULO)?,
            texto: fonte("SPEEDCONQUEST_FONTE_TEXTO", FONTE_TEXTO)?,
        })
    }

    /// Tamanho em px que dá a `f` um "x" de `altura` px (fontes diferentes têm
    /// proporções diferentes; medindo as minúsculas todas ficam do mesmo tamanho,
    /// inclusive as góticas, cujas maiúsculas são enormes).
    fn px_para_x(f: &FontVec, altura: f32) -> f32 {
        let g = f.glyph_id('x').with_scale(PxScale::from(100.0));
        match f.outline_glyph(g) {
            Some(og) if og.px_bounds().height() > 0.0 => 100.0 * altura / og.px_bounds().height(),
            _ => altura * 1.9,
        }
    }
}

fn largura_texto(f: &FontVec, px: f32, s: &str) -> f32 {
    let sf = f.as_scaled(PxScale::from(px));
    let mut w = 0.0;
    let mut ant = None;
    for ch in s.chars() {
        let id = sf.glyph_id(ch);
        if let Some(a) = ant {
            w += sf.kern(a, id);
        }
        w += sf.h_advance(id);
        ant = Some(id);
    }
    w
}

/// Corta o texto com "…" para caber em `max` pixels.
fn caber(f: &FontVec, px: f32, s: &str, max: f32) -> String {
    if largura_texto(f, px, s) <= max {
        return s.to_string();
    }
    let mut chars: Vec<char> = s.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let t: String = chars.iter().collect::<String>().trim_end().to_string() + "…";
        if largura_texto(f, px, &t) <= max {
            return t;
        }
    }
    "…".into()
}

/// Escreve texto com a linha de base em `y`, misturando por cima (src-over).
fn texto(pm: &mut Pixmap, f: &FontVec, px: f32, x: f32, y: f32, s: &str, cor: (u8, u8, u8)) {
    let sf = f.as_scaled(PxScale::from(px));
    let (w, h) = (pm.width(), pm.height());
    let pixels = pm.pixels_mut();
    let mut cx = x;
    let mut ant = None;
    for ch in s.chars() {
        let id = sf.glyph_id(ch);
        if let Some(a) = ant {
            cx += sf.kern(a, id);
        }
        let g = id.with_scale_and_position(PxScale::from(px), point(cx, y));
        cx += sf.h_advance(id);
        ant = Some(id);
        let Some(og) = f.outline_glyph(g) else {
            continue;
        };
        let b = og.px_bounds();
        og.draw(|gx, gy, cov| {
            let px_x = b.min.x as i64 + i64::from(gx);
            let px_y = b.min.y as i64 + i64::from(gy);
            if px_x < 0 || px_y < 0 || px_x >= i64::from(w) || px_y >= i64::from(h) {
                return;
            }
            // Dentro dos limites conferidos acima.
            let i = (px_y as usize) * (w as usize) + px_x as usize;
            let a = cov.clamp(0.0, 1.0);
            let d = pixels[i];
            let mix =
                |src: u8, dst: u8| (f32::from(src) * a + f32::from(dst) * (1.0 - a)).round() as u8;
            let na = mix(255, d.alpha());
            let (r, g, b) = (
                mix(cor.0, d.red()),
                mix(cor.1, d.green()),
                mix(cor.2, d.blue()),
            );
            if let Some(p) = PremultipliedColorU8::from_rgba(r.min(na), g.min(na), b.min(na), na) {
                pixels[i] = p;
            }
        });
    }
}

fn retangulo_redondo(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

fn pintar(pm: &mut Pixmap, path: Option<tiny_skia::Path>, cor: (u8, u8, u8, u8)) {
    if let Some(p) = path {
        let mut paint = Paint::default();
        paint.set_color_rgba8(cor.0, cor.1, cor.2, cor.3);
        paint.anti_alias = true;
        pm.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

/// Troféu desenhado por nós: círculo na cor do nível com uma taça escura.
fn trofeu(pm: &mut Pixmap, cx: f32, cy: f32, r: f32, nivel: Nivel) {
    trofeu_cor(pm, cx, cy, r, cor_nivel(nivel));
}

/// Ícone quadrado do troféu, em RGBA sem pré-multiplicar (para a janela).
/// `None` = conquista bloqueada (cinza).
pub fn icone_trofeu(lado: u32, nivel: Option<Nivel>) -> Option<Vec<u8>> {
    const BLOQUEADA: (u8, u8, u8) = (0x4A, 0x4A, 0x50);
    let mut pm = Pixmap::new(lado, lado)?;
    let c = lado as f32 / 2.0;
    trofeu_cor(&mut pm, c, c, c - 0.5, nivel.map_or(BLOQUEADA, cor_nivel));
    Some(
        pm.pixels()
            .iter()
            .flat_map(|p| {
                let d = p.demultiply();
                [d.red(), d.green(), d.blue(), d.alpha()]
            })
            .collect(),
    )
}

fn trofeu_cor(pm: &mut Pixmap, cx: f32, cy: f32, r: f32, (cr, cg, cb): (u8, u8, u8)) {
    pintar(pm, PathBuilder::from_circle(cx, cy, r), (cr, cg, cb, 255));
    let u = r / 26.0;
    let escuro = (24, 22, 20, 255);
    // Taça.
    let mut pb = PathBuilder::new();
    pb.move_to(cx - 11.0 * u, cy - 13.0 * u);
    pb.line_to(cx + 11.0 * u, cy - 13.0 * u);
    pb.line_to(cx + 11.0 * u, cy - 4.0 * u);
    pb.quad_to(cx + 11.0 * u, cy + 7.0 * u, cx, cy + 7.0 * u);
    pb.quad_to(cx - 11.0 * u, cy + 7.0 * u, cx - 11.0 * u, cy - 4.0 * u);
    pb.close();
    pintar(pm, pb.finish(), escuro);
    // Alças.
    let mut paint = Paint::default();
    paint.set_color_rgba8(escuro.0, escuro.1, escuro.2, escuro.3);
    paint.anti_alias = true;
    let traco = Stroke {
        width: 2.6 * u,
        ..Stroke::default()
    };
    for lado in [-1.0, 1.0] {
        if let Some(p) = PathBuilder::from_circle(cx + lado * 11.0 * u, cy - 7.0 * u, 4.8 * u) {
            pm.stroke_path(&p, &paint, &traco, Transform::identity(), None);
        }
    }
    // Haste e base.
    pintar(
        pm,
        Rect::from_xywh(cx - 2.2 * u, cy + 6.0 * u, 4.4 * u, 6.0 * u).map(PathBuilder::from_rect),
        escuro,
    );
    pintar(
        pm,
        retangulo_redondo(cx - 9.0 * u, cy + 12.0 * u, 18.0 * u, 4.5 * u, 1.5 * u),
        escuro,
    );
}

/// Desenha o popup. `s` = escala (1.0 em 768 linhas).
fn desenhar(av: &Aviso, s: f32, f: &Fontes) -> Result<Pixmap> {
    // Layout compacto: a largura acompanha o texto (entre LARGURA_MIN e LARGURA_MAX).
    let h = (60.0 * s).round();
    let x = 58.0 * s;
    let folga = 14.0 * s;
    let (tam1, tam2) = (
        Fontes::px_para_x(&f.titulo, 9.0 * s),
        Fontes::px_para_x(&f.texto, 7.0 * s),
    );
    let texto_max = (LARGURA_MAX * s).round() - x - folga;
    let titulo = caber(&f.titulo, tam1, &av.titulo, texto_max);
    let linha2 = caber(&f.texto, tam2, &av.linha2, texto_max);
    let conteudo =
        largura_texto(&f.titulo, tam1, &titulo).max(largura_texto(&f.texto, tam2, &linha2));
    let w = (x + conteudo + folga)
        .ceil()
        .clamp((LARGURA_MIN * s).round(), (LARGURA_MAX * s).round());
    let max = w - x - folga;

    let mut pm = Pixmap::new(w as u32, h as u32).context("tamanho do popup")?;
    pintar(
        &mut pm,
        retangulo_redondo(0.0, 0.0, w, h, 10.0 * s),
        (16, 16, 20, 255),
    );
    trofeu(&mut pm, 30.0 * s, h / 2.0, 20.0 * s, av.nivel);

    let y1 = if av.progresso.is_some() { 23.0 } else { 27.0 } * s;
    texto(&mut pm, &f.titulo, tam1, x, y1, &titulo, (245, 245, 245));
    texto(
        &mut pm,
        &f.texto,
        tam2,
        x,
        y1 + 17.0 * s,
        &linha2,
        (180, 180, 190),
    );

    if let Some(pct) = av.progresso {
        let (by, bh) = (h - 11.0 * s, 3.5 * s);
        pintar(
            &mut pm,
            retangulo_redondo(x, by, max, bh, bh / 2.0),
            (70, 70, 80, 255),
        );
        let cheio = max * f32::from(pct.min(100)) / 100.0;
        let (cr, cg, cb) = cor_nivel(av.nivel);
        if cheio > bh {
            pintar(
                &mut pm,
                retangulo_redondo(x, by, cheio, bh, bh / 2.0),
                (cr, cg, cb, 255),
            );
        }
    }
    Ok(pm)
}

/// Só desenha num PNG (para conferir o visual sem X).
pub fn salvar_png(av: &Aviso, escala: f32, saida: &std::path::Path) -> Result<()> {
    let pm = desenhar(av, escala, &Fontes::carregar()?)?;
    pm.save_png(saida).context("salvar png")?;
    Ok(())
}

/// Mostra o popup no `DISPLAY` atual por alguns segundos (bloqueia).
pub fn exibir(av: &Aviso) -> Result<()> {
    let fontes = Fontes::carregar()?;
    let (conn, tela) = x11rb::connect(None).context("conectar no X (DISPLAY)")?;
    let screen = &conn.setup().roots[tela];
    let (sw, sh) = (screen.width_in_pixels, screen.height_in_pixels);
    let escala = (f32::from(sh) / 768.0).max(0.75);
    let popup = desenhar(av, escala, &fontes)?;
    // Distância do canto superior direito (o usuário pediu bem encostado).
    let margem = (MARGEM * escala).round() as i32;
    let px = i32::from(sw) - popup.width() as i32 - margem;

    // O gamescope ignora a posição de janelas overlay (desenha em 0,0), então lá a
    // janela cobre a tela toda, transparente, com o popup no canto (como o mangoapp).
    // Overlay não recebe foco nem entrada. Fora do gamescope: janela pequena.
    let gamescope = conn
        .intern_atom(true, b"GAMESCOPE_EXTERNAL_OVERLAY")?
        .reply()?
        .atom
        != x11rb::NONE;
    let (pm, x, y) = if gamescope {
        let mut tela = Pixmap::new(u32::from(sw), u32::from(sh)).context("tamanho da tela")?;
        tela.draw_pixmap(
            px,
            margem,
            popup.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        (tela, 0i16, 0i16)
    } else {
        let x = i16::try_from(px).unwrap_or(0);
        let y = i16::try_from(margem).unwrap_or(0);
        (popup, x, y)
    };
    let (w, h) = (
        u16::try_from(pm.width()).context("popup largo demais")?,
        u16::try_from(pm.height()).context("popup alto demais")?,
    );

    // Visual de 32 bits (ARGB) para os cantos transparentes; senão, opaco.
    let argb = screen
        .allowed_depths
        .iter()
        .filter(|d| d.depth == 32)
        .flat_map(|d| d.visuals.iter())
        .find(|v| v.class == VisualClass::TRUE_COLOR)
        .map(|v| v.visual_id);
    let (prof, visual) = match argb {
        Some(v) => (32u8, v),
        None => (screen.root_depth, screen.root_visual),
    };

    let cmap = conn.generate_id()?;
    conn.create_colormap(ColormapAlloc::NONE, cmap, screen.root, visual)?;
    let pix = conn.generate_id()?;
    conn.create_pixmap(prof, pix, screen.root, w, h)?;
    let gc = conn.generate_id()?;
    conn.create_gc(gc, pix, &CreateGCAux::new())?;

    // tiny-skia: RGBA pré-multiplicado → X (little-endian): BGRA.
    let mut dados = pm.data().to_vec();
    for p in dados.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    let linha = usize::from(w) * 4;
    let por_req = ((conn.maximum_request_bytes() - 64) / linha).max(1);
    for (i, bloco) in dados.chunks(linha * por_req).enumerate() {
        let linhas = (bloco.len() / linha) as u16;
        let y0 = (i * por_req) as i16;
        conn.put_image(
            ImageFormat::Z_PIXMAP,
            pix,
            gc,
            w,
            linhas,
            0,
            y0,
            0,
            prof,
            bloco,
        )?;
    }

    let win = conn.generate_id()?;
    let aux = CreateWindowAux::new()
        .background_pixmap(pix)
        .border_pixel(0)
        .colormap(cmap)
        .override_redirect(1);
    conn.create_window(
        prof,
        win,
        screen.root,
        x,
        y,
        w,
        h,
        0,
        WindowClass::INPUT_OUTPUT,
        visual,
        &aux,
    )?;

    let atomo =
        |nome: &str| -> Result<u32> { Ok(conn.intern_atom(false, nome.as_bytes())?.reply()?.atom) };
    conn.change_property32(
        PropMode::REPLACE,
        win,
        atomo("GAMESCOPE_EXTERNAL_OVERLAY")?,
        AtomEnum::CARDINAL,
        &[1],
    )?;
    conn.change_property8(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"speedconquest",
    )?;
    let tipo = atomo("_NET_WM_WINDOW_TYPE")?;
    let notif = atomo("_NET_WM_WINDOW_TYPE_NOTIFICATION")?;
    conn.change_property32(PropMode::REPLACE, win, tipo, AtomEnum::ATOM, &[notif])?;
    conn.map_window(win)?;
    conn.flush()?;

    std::thread::sleep(DURACAO);
    conn.destroy_window(win)?;
    conn.free_pixmap(pix)?;
    conn.flush()?;
    Ok(())
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn fontes_embutidas_carregam_e_tem_acentos() {
        let f = Fontes::carregar().unwrap();
        for fonte in [&f.titulo, &f.texto] {
            for ch in "áãçéêíóôõúÁÇ$".chars() {
                assert_ne!(fonte.glyph_id(ch).0, 0, "sem glifo para {ch:?}");
            }
        }
    }

    #[test]
    fn desenha_sem_erro() {
        let av = Aviso {
            nivel: Nivel::Ouro,
            titulo: "Milionário do asfalto".into(),
            linha2: "Tenha US$ 1.000.000 na carreira.".into(),
            progresso: Some(50),
        };
        let pm = desenhar(&av, 1.0, &Fontes::carregar().unwrap()).unwrap();
        assert_eq!(pm.height(), 60);
        assert!((200..=340).contains(&pm.width()), "largura {}", pm.width());
    }
}

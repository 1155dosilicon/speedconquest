//! Janela com a lista de conquistas (egui): perfil, total, e cada conquista com
//! troféu, descrição, data do desbloqueio ou barra de progresso. Só lê o banco;
//! relê a cada `RELER_A_CADA` para acompanhar o vigia com o jogo aberto.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use eframe::egui::{
    self, Align, Color32, ColorImage, FontData, FontFamily, FontId, Layout, RichText,
    TextureHandle, TextureOptions, Vec2, load::SizedTexture,
};
use eframe::epaint::text::{FontInsert, FontPriority, InsertFontFamily};

use crate::banco::Banco;
use crate::config::{Conquista, Jogo, Nivel, Unidade};
use crate::popup;

const RELER_A_CADA: Duration = Duration::from_secs(2);

const FUNDO: Color32 = Color32::from_rgb(0x0A, 0x0A, 0x0C);
const CARTAO: Color32 = Color32::from_rgb(0x16, 0x16, 0x1A);
const CARTAO_FEITO: Color32 = Color32::from_rgb(0x1C, 0x1B, 0x1F);
const TEXTO: Color32 = Color32::from_rgb(0xEC, 0xEA, 0xE6);
const APAGADO: Color32 = Color32::from_rgb(0x8C, 0x8A, 0x92);
const TRILHO: Color32 = Color32::from_rgb(0x2A, 0x2A, 0x30);

/// Lado do ícone do troféu na lista (pixels lógicos).
const ICONE: f32 = 40.0;
/// Largura da coluna da direita (data ou barra de progresso).
const COLUNA: f32 = 190.0;

fn titulo() -> FontFamily {
    FontFamily::Name("titulo".into())
}

fn cor(n: Nivel) -> Color32 {
    let (r, g, b) = popup::cor_nivel(n);
    Color32::from_rgb(r, g, b)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filtro {
    Todas,
    Faltam,
    Feitas,
}

/// Como mostrar o progresso de uma conquista.
struct Andamento {
    /// 0.0–1.0.
    fracao: f32,
    texto: String,
}

pub struct Janela {
    banco: Banco,
    jogos: Vec<Jogo>,
    jogo: usize,
    perfis: Vec<String>,
    perfil: Option<String>,
    desbloqueios: HashMap<String, String>,
    maiores: HashMap<String, i64>,
    marcos: HashMap<String, u8>,
    filtro: Filtro,
    lido: Option<Instant>,
    erro: Option<String>,
    /// Bronze, prata, ouro, platina, bloqueada.
    icones: Vec<TextureHandle>,
}

fn indice_icone(n: Option<Nivel>) -> usize {
    match n {
        Some(Nivel::Bronze) => 0,
        Some(Nivel::Prata) => 1,
        Some(Nivel::Ouro) => 2,
        Some(Nivel::Platina) => 3,
        None => 4,
    }
}

/// Lado em pixels físicos dos ícones (nítidos em tela com escala 2).
const ICONE_PX: u32 = 96;

pub fn abrir(banco: Banco, jogos: Vec<Jogo>) -> Result<()> {
    let icone = popup::icone_trofeu(64, Some(Nivel::Ouro)).map(|rgba| egui::IconData {
        rgba,
        width: 64,
        height: 64,
    });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("speedconquest")
        .with_app_id("speedconquest")
        .with_inner_size([760.0, 720.0])
        .with_min_inner_size([520.0, 360.0]);
    if let Some(i) = icone {
        viewport = viewport.with_icon(Arc::new(i));
    }
    let opcoes = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "speedconquest",
        opcoes,
        Box::new(move |cc| {
            configurar(&cc.egui_ctx);
            let icones = [
                Some(Nivel::Bronze),
                Some(Nivel::Prata),
                Some(Nivel::Ouro),
                Some(Nivel::Platina),
                None,
            ]
            .into_iter()
            .map(|n| {
                let rgba = popup::icone_trofeu(ICONE_PX, n).unwrap_or_default();
                let lado = ICONE_PX as usize;
                let img = if rgba.len() == lado * lado * 4 {
                    ColorImage::from_rgba_unmultiplied([lado, lado], &rgba)
                } else {
                    ColorImage::filled([1, 1], Color32::TRANSPARENT)
                };
                cc.egui_ctx.load_texture(
                    format!("trofeu{}", indice_icone(n)),
                    img,
                    TextureOptions::LINEAR,
                )
            })
            .collect();
            Ok(Box::new(Janela {
                banco,
                jogos,
                jogo: 0,
                perfis: Vec::new(),
                perfil: None,
                desbloqueios: HashMap::new(),
                maiores: HashMap::new(),
                marcos: HashMap::new(),
                filtro: Filtro::Todas,
                lido: None,
                erro: None,
                icones,
            }))
        }),
    )
    .map_err(|e| anyhow!("janela: {e}"))
}

fn configurar(ctx: &egui::Context) {
    ctx.add_font(FontInsert::new(
        "titillium",
        FontData::from_static(popup::FONTE_TEXTO),
        vec![InsertFontFamily {
            family: FontFamily::Proportional,
            priority: FontPriority::Highest,
        }],
    ));
    ctx.add_font(FontInsert::new(
        "grenze",
        FontData::from_static(popup::FONTE_TITULO),
        vec![InsertFontFamily {
            family: titulo(),
            priority: FontPriority::Highest,
        }],
    ));
    // Acentos/símbolos que faltarem na Grenze caem na Titillium.
    ctx.add_font(FontInsert::new(
        "titillium-reserva",
        FontData::from_static(popup::FONTE_TEXTO),
        vec![InsertFontFamily {
            family: titulo(),
            priority: FontPriority::Lowest,
        }],
    ));
    let mut v = egui::Visuals::dark();
    v.panel_fill = FUNDO;
    v.window_fill = FUNDO;
    v.extreme_bg_color = TRILHO;
    v.override_text_color = Some(TEXTO);
    v.selection.bg_fill = Color32::from_rgb(0x5A, 0x48, 0x14);
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        for (estilo, tam) in [
            (egui::TextStyle::Body, 16.0),
            (egui::TextStyle::Button, 16.0),
            (egui::TextStyle::Small, 13.0),
        ] {
            s.text_styles
                .insert(estilo, FontId::new(tam, FontFamily::Proportional));
        }
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
    });
}

impl Janela {
    fn jogo(&self) -> &Jogo {
        &self.jogos[self.jogo]
    }

    fn reler(&mut self) {
        self.lido = Some(Instant::now());
        if let Err(e) = self.reler_banco() {
            self.erro = Some(format!("{e:#}"));
        } else {
            self.erro = None;
        }
    }

    fn reler_banco(&mut self) -> Result<()> {
        let id = self.jogo().id.clone();
        self.perfis = self.banco.perfis(&id)?;
        if self
            .perfil
            .as_ref()
            .is_none_or(|p| !self.perfis.contains(p))
        {
            self.perfil = self
                .banco
                .perfil_recente(&id)?
                .or_else(|| self.perfis.first().cloned());
        }
        match &self.perfil {
            Some(p) => {
                self.desbloqueios = self.banco.desbloqueios(&id, p)?;
                // Estatísticas guardadas (odômetro, ganhos…) também valem: são de antes
                // da tabela `maior` existir ou ainda não foram copiadas para ela.
                let mut m = self.banco.estatisticas(&id, p)?;
                for (k, v) in self.banco.maiores(&id, p)? {
                    let e = m.entry(k).or_insert(v);
                    *e = (*e).max(v);
                }
                self.maiores = m;
                self.marcos = self.banco.marcos(&id, p)?;
            }
            None => {
                self.desbloqueios.clear();
                self.maiores.clear();
                self.marcos.clear();
            }
        }
        Ok(())
    }

    fn andamento(&self, c: &Conquista) -> Andamento {
        let jogo = self.jogo();
        let Some(p) = &c.progresso else {
            // Platina: as outras conquistas.
            let outras = jogo.conquistas.iter().filter(|o| o.id != c.id);
            let total = outras.clone().count();
            let feitas = outras
                .filter(|o| self.desbloqueios.contains_key(&o.id))
                .count();
            return Andamento {
                fracao: fracao(feitas as i64, total as i64),
                texto: format!("{feitas} / {total} conquistas"),
            };
        };
        let formatar = |v: i64| match jogo.unidade.get(&p.var) {
            Some(u) => u.formatar(v),
            None => Unidade {
                texto: String::new(),
                antes: false,
                divisor: 0,
            }
            .formatar(v)
            .trim_end()
            .to_string(),
        };
        if let Some(&v) = self.maiores.get(&p.var) {
            let recorde = if c.marcos { "" } else { "recorde " };
            Andamento {
                fracao: fracao(v, p.meta),
                texto: format!("{recorde}{} / {}", formatar(v), formatar(p.meta)),
            }
        } else if let Some(&pct) = self.marcos.get(&c.id) {
            Andamento {
                fracao: f32::from(pct) / 100.0,
                texto: format!("{pct}% ou mais"),
            }
        } else {
            Andamento {
                fracao: 0.0,
                texto: format!("meta {}", formatar(p.meta)),
            }
        }
    }

    fn cabecalho(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("speedconquest")
                    .family(titulo())
                    .size(30.0)
                    .color(cor(Nivel::Ouro)),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut trocou = false;
                if !self.perfis.is_empty() {
                    let atual = self.perfil.clone().unwrap_or_default();
                    egui::ComboBox::from_id_salt("perfil")
                        .selected_text(atual)
                        .show_ui(ui, |ui| {
                            for p in &self.perfis {
                                let sel = self.perfil.as_ref() == Some(p);
                                if ui.selectable_label(sel, p).clicked() && !sel {
                                    self.perfil = Some(p.clone());
                                    trocou = true;
                                }
                            }
                        });
                    ui.label(RichText::new("perfil").color(APAGADO));
                }
                if self.jogos.len() > 1 {
                    let nomes: Vec<String> = self.jogos.iter().map(|j| j.nome.clone()).collect();
                    egui::ComboBox::from_id_salt("jogo")
                        .selected_text(nomes[self.jogo].clone())
                        .show_ui(ui, |ui| {
                            for (i, n) in nomes.iter().enumerate() {
                                if ui.selectable_label(i == self.jogo, n).clicked()
                                    && i != self.jogo
                                {
                                    self.jogo = i;
                                    self.perfil = None;
                                    trocou = true;
                                }
                            }
                        });
                } else {
                    ui.label(RichText::new(&self.jogo().nome).color(APAGADO));
                }
                if trocou {
                    self.reler();
                }
            });
        });
        ui.add_space(4.0);

        // Resumo: total e por nível.
        let jogo = self.jogo();
        let total = jogo.conquistas.len();
        let feitas = jogo
            .conquistas
            .iter()
            .filter(|c| self.desbloqueios.contains_key(&c.id))
            .count();
        let f = fracao(feitas as i64, total as i64);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{feitas} de {total}"))
                    .size(20.0)
                    .strong(),
            );
            ui.label(RichText::new(format!("{:.0}%", f * 100.0)).color(APAGADO));
            ui.add_space(12.0);
            for n in [Nivel::Bronze, Nivel::Prata, Nivel::Ouro, Nivel::Platina] {
                let do_nivel = jogo.conquistas.iter().filter(|c| c.nivel == n);
                let t = do_nivel.clone().count();
                if t == 0 {
                    continue;
                }
                let k = do_nivel
                    .filter(|c| self.desbloqueios.contains_key(&c.id))
                    .count();
                ui.add(
                    egui::Image::new(SizedTexture::from(&self.icones[indice_icone(Some(n))]))
                        .fit_to_exact_size(Vec2::splat(18.0)),
                );
                ui.label(format!("{k}/{t}"));
                ui.add_space(6.0);
            }
        });
        barra(ui, ui.available_width(), f, cor(Nivel::Ouro));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            for (filtro, nome) in [
                (Filtro::Todas, "Todas"),
                (Filtro::Faltam, "Faltam"),
                (Filtro::Feitas, "Feitas"),
            ] {
                ui.selectable_value(&mut self.filtro, filtro, nome);
            }
        });
        ui.add_space(6.0);
    }

    fn cartao(&self, ui: &mut egui::Ui, c: &Conquista) {
        let feita = self.desbloqueios.get(&c.id);
        let icone = &self.icones[indice_icone(feita.map(|_| c.nivel))];
        egui::Frame::new()
            .fill(if feita.is_some() {
                CARTAO_FEITO
            } else {
                CARTAO
            })
            .corner_radius(8.0)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Image::new(SizedTexture::from(icone))
                            .fit_to_exact_size(Vec2::splat(ICONE)),
                    );
                    ui.add_space(6.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.allocate_ui_with_layout(
                            Vec2::new(COLUNA, ICONE),
                            Layout::top_down(Align::Max),
                            |ui| self.coluna(ui, c, feita),
                        );
                        ui.add_space(10.0);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            let nome = RichText::new(&c.nome).family(titulo()).size(21.0);
                            ui.label(if feita.is_some() {
                                nome.color(TEXTO)
                            } else {
                                nome.color(Color32::from_rgb(0xC8, 0xC6, 0xCC))
                            });
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&c.descricao).size(14.5).color(APAGADO),
                                )
                                .wrap(),
                            );
                        });
                    });
                });
            });
    }

    fn coluna(&self, ui: &mut egui::Ui, c: &Conquista, feita: Option<&String>) {
        if let Some(quando) = feita {
            ui.label(RichText::new("desbloqueada").color(cor(c.nivel)));
            ui.label(RichText::new(quando).small().color(APAGADO));
            return;
        }
        let a = self.andamento(c);
        ui.add_space(6.0);
        barra(ui, COLUNA, a.fracao, cor(c.nivel));
        ui.label(RichText::new(a.texto).small().color(APAGADO));
    }
}

/// Barra fina de progresso (a do egui desenha um ponto em 0%).
fn barra(ui: &mut egui::Ui, largura: f32, fracao: f32, cheia: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(largura, 6.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, TRILHO);
    if fracao > 0.0 {
        let mut r = rect;
        r.set_width((rect.width() * fracao).max(6.0));
        p.rect_filled(r, 3.0, cheia);
    }
}

fn fracao(v: i64, meta: i64) -> f32 {
    if meta <= 0 {
        return 0.0;
    }
    // Só para desenhar a barra: precisão de f32 sobra.
    #[allow(clippy::cast_precision_loss)] // ver acima
    let f = v.clamp(0, meta) as f32 / meta as f32;
    f
}

impl eframe::App for Janela {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.lido.is_none_or(|t| t.elapsed() >= RELER_A_CADA) {
            self.reler();
        }
        ui.ctx().request_repaint_after(RELER_A_CADA);
        egui::Panel::top("cabecalho")
            .frame(
                egui::Frame::new()
                    .fill(FUNDO)
                    .inner_margin(egui::Margin::symmetric(16, 6)),
            )
            .show(ui, |ui| self.cabecalho(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(FUNDO)
                    .inner_margin(egui::Margin::symmetric(16, 4)),
            )
            .show(ui, |ui| {
                if let Some(e) = &self.erro {
                    ui.colored_label(Color32::from_rgb(0xE0, 0x60, 0x50), e);
                }
                if self.perfil.is_none() {
                    ui.add_space(24.0);
                    ui.label(
                        RichText::new(
                            "Nenhum perfil ainda. Abra o jogo, carregue um save e jogue: \
                             o progresso aparece aqui.",
                        )
                        .color(APAGADO),
                    );
                    return;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let jogo = self.jogo();
                        for c in &jogo.conquistas {
                            let feita = self.desbloqueios.contains_key(&c.id);
                            let mostrar = match self.filtro {
                                Filtro::Todas => true,
                                Filtro::Faltam => !feita,
                                Filtro::Feitas => feita,
                            };
                            if mostrar {
                                self.cartao(ui, c);
                            }
                        }
                        ui.add_space(8.0);
                    });
            });
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn fracao_limita() {
        assert!((fracao(50, 100) - 0.5).abs() < 1e-6);
        assert!((fracao(500, 100) - 1.0).abs() < 1e-6);
        assert!(fracao(-3, 100).abs() < 1e-6);
        assert!(fracao(1, 0).abs() < 1e-6);
    }

    #[test]
    fn icones_tem_tamanho_certo() {
        for n in [Some(Nivel::Bronze), Some(Nivel::Platina), None] {
            assert_eq!(
                popup::icone_trofeu(ICONE_PX, n).unwrap().len(),
                (ICONE_PX * ICONE_PX * 4) as usize
            );
        }
    }
}

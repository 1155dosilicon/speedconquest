// Decompila funções nos endereços passados (hex) e lista quem as chama.
// Uso: ghidra-headless <proj> <nome> -process SPEED2.EXE -noanalysis -postScript Decomp.java 4AB780 ...
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.address.*;
import ghidra.program.model.symbol.*;

public class Decomp extends GhidraScript {
    @Override
    public void run() throws Exception {
        DecompInterface d = new DecompInterface();
        d.openProgram(currentProgram);
        for (String a : getScriptArgs()) {
            Address ad = toAddr(Long.parseLong(a, 16));
            Function f = getFunctionContaining(ad);
            if (f == null) { f = createFunction(ad, null); }
            if (f == null) { println("sem função em " + a); continue; }
            println("==== " + f.getName() + " @ " + f.getEntryPoint());
            for (Reference r : getReferencesTo(f.getEntryPoint())) {
                Function c = getFunctionContaining(r.getFromAddress());
                println("  chamada de " + r.getFromAddress() + (c != null ? " (" + c.getName() + ")" : ""));
            }
            DecompileResults res = d.decompileFunction(f, 120, monitor);
            println(res.getDecompiledFunction() != null ? res.getDecompiledFunction().getC() : "falhou");
        }
    }
}

// Lista quem referencia endereços de dados (hex) e em qual função.
// Uso: ghidra-headless <proj> <nome> -process SPEED2.EXE -noanalysis -readOnly -postScript Refs.java 82BE80 ...
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import ghidra.program.model.address.*;
import ghidra.program.model.symbol.*;

public class Refs extends GhidraScript {
    @Override
    public void run() throws Exception {
        for (String a : getScriptArgs()) {
            Address base = toAddr(Long.parseLong(a, 16));
            for (int off = 0; off < 16; off += 4) {
                Address ad = base.add(off);
                for (Reference r : getReferencesTo(ad)) {
                    Function c = getFunctionContaining(r.getFromAddress());
                    Instruction ins = getInstructionAt(r.getFromAddress());
                    println(ad + " <- " + r.getFromAddress() + " " + r.getReferenceType() + " "
                        + (c != null ? c.getName() + "@" + c.getEntryPoint() : "?") + "  " + (ins != null ? ins.toString() : ""));
                }
            }
        }
    }
}

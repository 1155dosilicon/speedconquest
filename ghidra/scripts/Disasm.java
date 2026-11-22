// Desmonta funções (hex) inteiras. Uso: ... -postScript Disasm.java 4bf9e0 ...
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import ghidra.program.model.address.*;

public class Disasm extends GhidraScript {
    @Override
    public void run() throws Exception {
        for (String a : getScriptArgs()) {
            Address ad = toAddr(Long.parseLong(a, 16));
            Function f = getFunctionContaining(ad);
            if (f == null) { disassemble(ad); f = createFunction(ad, null); }
            if (f == null) { println("sem função em " + a); continue; }
            println("==== " + f.getName());
            InstructionIterator it = currentProgram.getListing().getInstructions(f.getBody(), true);
            while (it.hasNext()) { Instruction i = it.next(); println(i.getAddress() + "  " + i); }
        }
    }
}

package digital.quebracho.lapacho.ime

import digital.quebracho.lapacho.ime.LapachoIme.Companion.withAcute
import org.junit.Assert.assertEquals
import org.junit.Test

class AccentTest {
    @Test fun acuteOnVowelsKeepsCase() {
        assertEquals("áéíóú", "aeiou".map { withAcute(it.toString()) }.joinToString(""))
        assertEquals("Á", withAcute("A"))
    }

    @Test fun acuteOnAnythingElseIsANoOp() {
        assertEquals("x", withAcute("x"))
        assertEquals("ñ", withAcute("ñ"))
        assertEquals("?", withAcute("?"))
    }
}

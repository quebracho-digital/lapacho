package digital.quebracho.lapacho.ime

import digital.quebracho.lapacho.ime.LapachoIme.Companion.meantKey
import org.junit.Assert.assertEquals
import org.junit.Test

class MeantKeyTest {
    private val words = listOf("hola", "como", "tiempo", "bien")
    private fun meant(word: String, key: Char, fx: Float, fy: Float, left: Char? = null, right: Char? = null, overSpace: Boolean = false) =
        meantKey(word, key, fx, fy, left, right, overSpace, { p -> words.any { it.startsWith(p) } }, { it in words })

    @Test fun lowTapUnderACompleteWordIsASpace() {
        assertEquals(' ', meant("hola", 'b', 0.5f, 0.8f, 'v', 'n', overSpace = true))
        assertEquals(' ', meant("Hola", 'n', 0.5f, 0.8f, 'b', 'm', overSpace = true))
    }

    @Test fun aWordThatCanGoOnKeepsTheKey() {
        assertEquals('b', meant("", 'b', 0.5f, 0.9f, overSpace = true))
        assertEquals('b', meant("hola", 'b', 0.5f, 0.3f, overSpace = true))
        assertEquals('b', meant("xyz", 'b', 0.5f, 0.9f, overSpace = true))
        assertEquals('o', meant("c", 'o', 0.1f, 0.5f, 'i', 'p'))
    }

    @Test fun anEdgeTapGoesToTheNeighbourThatFits() {
        assertEquals('o', meant("c", 'p', 0.1f, 0.5f, 'o', null))
        assertEquals('p', meant("tiem", 'o', 0.9f, 0.5f, 'i', 'p'))
        assertEquals('o', meant("tiem", 'o', 0.5f, 0.5f, 'i', 'p'))
    }
}

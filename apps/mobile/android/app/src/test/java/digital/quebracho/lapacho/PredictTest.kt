package digital.quebracho.lapacho

import org.junit.Assert.assertEquals
import org.junit.Test

class PredictTest {
    @Test fun takesTheLettersEndingAtTheCursor() {
        assertEquals("mun", currentWord("hola mun"))
        assertEquals("canción", currentWord("una canción"))
        assertEquals("qué", currentWord("¿qué"))
    }

    @Test fun thereIsNoWordAfterASpaceOrAPunctuationMark() {
        assertEquals("", currentWord("hola "))
        assertEquals("", currentWord("hola."))
        assertEquals("", currentWord(""))
        assertEquals("", currentWord(null))
    }
}

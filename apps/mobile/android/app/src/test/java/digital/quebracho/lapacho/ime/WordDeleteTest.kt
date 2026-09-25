package digital.quebracho.lapacho.ime

import digital.quebracho.lapacho.ime.LapachoIme.Companion.wordDeleteLength
import org.junit.Assert.assertEquals
import org.junit.Test

class WordDeleteTest {
    @Test fun takesTheWordBeforeTheCursor() {
        assertEquals("mundo".length, wordDeleteLength("hola mundo"))
    }

    @Test fun takesTheSpacesAfterTheWordToo() {
        assertEquals("mundo  ".length, wordDeleteLength("hola mundo  "))
        assertEquals("hola\n".length, wordDeleteLength("hola\n"))
    }

    @Test fun punctuationGoesWithItsWord() {
        assertEquals("¿qué?".length, wordDeleteLength("dijo ¿qué?"))
    }

    @Test fun oneWordOrNothing() {
        assertEquals(4, wordDeleteLength("hola"))
        assertEquals(0, wordDeleteLength(""))
        assertEquals(3, wordDeleteLength("   "))
    }
}

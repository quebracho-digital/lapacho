package digital.quebracho.lapacho

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
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

    @Test fun readsTheHeader() {
        val h = parseHeader("$DICT_MAGIC\n#lang pt\n#name Português\n#alternates c:ç a:ãáâ\nde 10\n#lang xx\n")
        assertEquals(DictHeader("pt", "Português", mapOf("c" to "ç", "a" to "ãáâ")), h)
    }

    @Test fun theNameDefaultsToTheLanguage() {
        assertEquals("en", parseHeader("$DICT_MAGIC\n#lang en\nthe 1").name)
    }

    @Test fun refusesWhatIsNotADictionary() {
        // No magic line: any text file the picker could hand us.
        assertThrows(IllegalArgumentException::class.java) { parseHeader("#lang en\nthe 1") }
        // A language that would climb out of the dictionary directory.
        assertThrows(IllegalArgumentException::class.java) { parseHeader("$DICT_MAGIC\n#lang ../../db\n") }
        assertThrows(IllegalArgumentException::class.java) { parseHeader("$DICT_MAGIC\nthe 1") }
        assertThrows(IllegalArgumentException::class.java) { parseHeader("$DICT_MAGIC\n#lang en\n#alternates ab:c\n") }
        assertThrows(IllegalArgumentException::class.java) { parseHeader("$DICT_MAGIC\n#lang en\n#alternates n\n") }
    }

    @Test fun alternatesMergeInOrderEachOnce() {
        val es = DictHeader("es", "Español", mapOf("n" to "ñ"))
        val pt = DictHeader("pt", "Português", mapOf("n" to "ñ", "a" to "ã"))
        assertEquals(mapOf("a" to "@ã", "n" to "ñ"), keyAlternates(listOf(es, pt), mapOf("a" to "@")))
    }
}

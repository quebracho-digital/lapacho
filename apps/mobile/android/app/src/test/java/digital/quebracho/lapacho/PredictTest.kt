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

    @Test fun readsTheLetterRows() {
        val h = parseHeader("$DICT_MAGIC\n#lang fr\n#rows azertyuiop qsdfghjklm wxcvbn\nde 10\n")
        assertEquals(listOf("azertyuiop", "qsdfghjklm", "wxcvbn"), h.rows)
        // Any script with letters; one without case is fine.
        assertEquals(listOf("קראטוןםפ", "שדגכעיחלךף", "זסבהנמצתץ"),
            parseHeader("$DICT_MAGIC\n#lang he\n#rows קראטוןםפ שדגכעיחלךף זסבהנמצתץ\n").rows)
        assertEquals("no #rows: the keyboard's own", null, parseHeader("$DICT_MAGIC\n#lang es\nde 1").rows)
    }

    @Test fun refusesRowsTheKeyboardCannotDraw() {
        for (rows in listOf("qwertyuiop", "a b c d e", "qwerty qwerty", "QWERTY asdf", "qwe1 asd", "qwertyuiopasd zxc")) {
            assertThrows(rows, IllegalArgumentException::class.java) { parseHeader("$DICT_MAGIC\n#lang xx\n#rows $rows\n") }
        }
    }

    @Test fun languagesThatShareRowsShareALayout() {
        val es = DictHeader("es", "es", emptyMap())
        val fr = DictHeader("fr", "fr", emptyMap(), listOf("azertyuiop", "qsdfghjklm", "wxcvbn"))
        val en = DictHeader("en", "en", emptyMap())
        val be = DictHeader("be", "be", emptyMap(), listOf("azertyuiop", "qsdfghjklm", "wxcvbn"))
        assertEquals(listOf(listOf(es, en), listOf(fr, be)), layouts(listOf(es, fr, en, be)) { it }.map { it.second })
    }

    @Test fun alternatesMergeInOrderEachOnce() {
        val es = DictHeader("es", "Español", mapOf("n" to "ñ"))
        val pt = DictHeader("pt", "Português", mapOf("n" to "ñ", "a" to "ã"))
        assertEquals(mapOf("a" to "@ã", "n" to "ñ"), keyAlternates(listOf(es, pt), mapOf("a" to "@")))
    }

    @Test fun aBundledLanguageIsOffWhenRemovedOrReplaced() {
        assertEquals(listOf("es", "en"), activeBundled(listOf("es", "en"), emptySet(), emptySet()))
        assertEquals(listOf("en"), activeBundled(listOf("es", "en"), setOf("es"), emptySet()))
        // An imported English replaces the bundled one instead of doubling it.
        assertEquals(listOf("es"), activeBundled(listOf("es", "en"), emptySet(), setOf("en")))
    }

    /** Each bundled asset parses as its own language, and English is the published list byte for byte. */
    @Test fun theBundledDictionariesAreWhole() {
        for (lang in BUNDLED_LANGUAGES) {
            val text = java.io.File("src/main/assets/dict/$lang.txt").readText()
            assertEquals(lang, parseHeader(text).lang)
        }
        assertEquals(
            java.io.File("../dictionaries/en.txt").readText(),
            java.io.File("src/main/assets/dict/en.txt").readText(),
        )
    }

    /**
     * Every official hash is the hash of a committed file, and every
     * committed dictionary is official — so the file on the release page and
     * the list inside the APK cannot drift apart unnoticed.
     */
    @Test fun officialHashesMatchTheCommittedDictionaries() {
        // Unit tests run from the module directory (app/).
        val dir = java.io.File("../dictionaries")
        val committed = dir.listFiles { f -> f.name.endsWith(".txt") && !f.name.startsWith("NOTICE") }!!
            .associate { f ->
                val sha = java.security.MessageDigest.getInstance("SHA-256").digest(f.readBytes())
                    .joinToString("") { "%02x".format(it) }
                sha to parseHeader(f.readText()).lang
            }
        assertEquals(committed, OFFICIAL_DICTIONARIES)
    }
}

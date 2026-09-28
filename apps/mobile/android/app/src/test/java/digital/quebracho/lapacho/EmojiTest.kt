package digital.quebracho.lapacho

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class EmojiTest {
    private val sample = """
        # header comment
        #group 😀
        😀	cara sonriente grinning face
        😂	cara llorando de risa joy
        #group 🐻
        🤱	lactancia materna
        🐶	perro dog
        🧉	bebida mate
        🦖	t-rex
        #group 🏁
    """.trimIndent()

    @Test fun groupsInOrderAndEmptyOnesDropped() {
        val groups = parseEmoji(sample)
        assertEquals(listOf("😀", "🐻"), groups.map { it.icon })
        assertEquals(listOf("😀", "😂"), groups[0].emojis.map { it.glyph })
    }

    @Test fun dropsWhatThePhoneCannotDraw() {
        val groups = parseEmoji(sample) { it != "🦖" }
        assertEquals(listOf("🤱", "🐶", "🧉"), groups[1].emojis.map { it.glyph })
    }

    @Test fun searchesSpanishAndEnglishIgnoringAccents() {
        val groups = parseEmoji(sample)
        assertEquals(listOf("🐶"), searchEmoji(groups, "PERRO", 10).map { it.glyph })
        assertEquals(listOf("😂"), searchEmoji(groups, "risa", 10).map { it.glyph })
        assertEquals(listOf("🐶"), searchEmoji(groups, "dog", 10).map { it.glyph })
        assertEquals(listOf("😀"), searchEmoji(groups, "cara", 1).map { it.glyph })
    }

    @Test fun wholeWordsRankBeforePartsOfWords() {
        assertEquals(listOf("🧉", "🤱"), searchEmoji(parseEmoji(sample), "mate", 10).map { it.glyph })
    }

    /** The committed list parses, and the most typed ones are findable in Spanish. */
    @Test fun theBundledListIsWhole() {
        // Unit tests run from the module directory (app/).
        val groups = parseEmoji(java.io.File("src/main/assets/emoji.tsv").readText())
        assertEquals(9, groups.size)
        assertTrue(groups.sumOf { it.emojis.size } > 1500)
        val first = { q: String -> searchEmoji(groups, q, 1).single().glyph }
        assertEquals("❤️", first("corazon rojo"))
        assertEquals("👍", first("pulgar arriba"))
        assertEquals("🔥", first("fuego"))
        assertEquals("🧉", first("mate"))
        assertEquals("🇦🇷", first("bandera argentina"))
    }
}

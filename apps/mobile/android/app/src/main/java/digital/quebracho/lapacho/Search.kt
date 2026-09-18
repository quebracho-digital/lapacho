package digital.quebracho.lapacho

import java.text.Normalizer

/**
 * Whether [text] matches [query]: every word of the query appears somewhere,
 * ignoring case and accents ("cancion" finds "Canción"). An empty query
 * matches everything.
 */
fun matchesQuery(text: String, query: String): Boolean {
    val haystack = fold(text)
    return fold(query).split(' ').filter { it.isNotEmpty() }.all { it in haystack }
}

private val MARKS = Regex("\\p{Mn}+")

private fun fold(s: String): String = MARKS.replace(Normalizer.normalize(s, Normalizer.Form.NFD), "").lowercase()

package digital.quebracho.lapacho

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SearchTest {
    @Test fun ignoresCaseAndAccents() {
        assertTrue(matchesQuery("Canción de cuna", "cancion"))
        assertTrue(matchesQuery("cancion de cuna", "CANCIÓN"))
    }

    @Test fun everyWordMustAppearInAnyOrder() {
        assertTrue(matchesQuery("https://web.fishman.work/lapacho.apk", "apk fishman"))
        assertFalse(matchesQuery("https://web.fishman.work/lapacho.apk", "apk google"))
    }

    @Test fun emptyQueryMatchesEverything() {
        assertTrue(matchesQuery("anything", ""))
        assertTrue(matchesQuery("anything", "   "))
    }
}

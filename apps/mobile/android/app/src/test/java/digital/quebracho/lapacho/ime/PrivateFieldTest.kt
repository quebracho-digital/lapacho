package digital.quebracho.lapacho.ime

import android.text.InputType.TYPE_CLASS_NUMBER
import android.text.InputType.TYPE_CLASS_TEXT
import android.text.InputType.TYPE_NUMBER_VARIATION_PASSWORD
import android.text.InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
import android.text.InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
import android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD
import android.text.InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
import android.text.InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD
import android.view.inputmethod.EditorInfo.IME_ACTION_DONE
import android.view.inputmethod.EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING
import digital.quebracho.lapacho.ime.LapachoIme.Companion.isPrivateField
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PrivateFieldTest {
    @Test fun passwordsAndPinsArePrivate() {
        for (v in listOf(TYPE_TEXT_VARIATION_PASSWORD, TYPE_TEXT_VARIATION_WEB_PASSWORD, TYPE_TEXT_VARIATION_VISIBLE_PASSWORD)) {
            assertTrue(isPrivateField(TYPE_CLASS_TEXT or v or TYPE_TEXT_FLAG_NO_SUGGESTIONS, IME_ACTION_DONE))
        }
        assertTrue(isPrivateField(TYPE_CLASS_NUMBER or TYPE_NUMBER_VARIATION_PASSWORD, 0))
    }

    @Test fun incognitoIsPrivate() {
        assertTrue(isPrivateField(TYPE_CLASS_TEXT, IME_ACTION_DONE or IME_FLAG_NO_PERSONALIZED_LEARNING))
    }

    @Test fun ordinaryFieldsAreNot() {
        assertFalse(isPrivateField(TYPE_CLASS_TEXT, 0))
        assertFalse(isPrivateField(TYPE_CLASS_TEXT or TYPE_TEXT_VARIATION_EMAIL_ADDRESS, IME_ACTION_DONE))
        assertFalse(isPrivateField(TYPE_CLASS_NUMBER, 0))
    }
}

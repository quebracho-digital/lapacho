package digital.quebracho.lapacho.ime

import android.text.InputType.TYPE_CLASS_DATETIME
import android.text.InputType.TYPE_CLASS_NUMBER
import android.text.InputType.TYPE_CLASS_PHONE
import android.text.InputType.TYPE_CLASS_TEXT
import android.text.InputType.TYPE_NUMBER_FLAG_DECIMAL
import android.text.InputType.TYPE_NUMBER_VARIATION_PASSWORD
import android.text.InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
import digital.quebracho.lapacho.ime.LapachoIme.Companion.initialLayer
import digital.quebracho.lapacho.ime.LapachoIme.Layer
import org.junit.Assert.assertEquals
import org.junit.Test

class InitialLayerTest {
    @Test fun numericFieldsOpenOnTheNumbers() {
        // A transfer amount, a PIN, a phone number, a date.
        assertEquals(Layer.SYMBOLS, initialLayer(TYPE_CLASS_NUMBER or TYPE_NUMBER_FLAG_DECIMAL))
        assertEquals(Layer.SYMBOLS, initialLayer(TYPE_CLASS_NUMBER or TYPE_NUMBER_VARIATION_PASSWORD))
        assertEquals(Layer.SYMBOLS, initialLayer(TYPE_CLASS_PHONE))
        assertEquals(Layer.SYMBOLS, initialLayer(TYPE_CLASS_DATETIME))
    }

    @Test fun everythingElseOpensOnTheLetters() {
        assertEquals(Layer.LETTERS, initialLayer(TYPE_CLASS_TEXT))
        assertEquals(Layer.LETTERS, initialLayer(TYPE_CLASS_TEXT or TYPE_TEXT_VARIATION_EMAIL_ADDRESS))
        assertEquals(Layer.LETTERS, initialLayer(0))
    }
}

package digital.quebracho.lapacho.ime

import digital.quebracho.lapacho.ime.LapachoIme.Companion.nextShift
import digital.quebracho.lapacho.ime.LapachoIme.Shift
import org.junit.Assert.assertEquals
import org.junit.Test

class ShiftTest {
    @Test fun doubleTapLocksAndThirdTapReleases() {
        val once = nextShift(Shift.OFF, 10_000)
        assertEquals(Shift.ONCE, once)
        val locked = nextShift(once, 200)
        assertEquals(Shift.LOCKED, locked)
        assertEquals(Shift.OFF, nextShift(locked, 200))
    }

    @Test fun slowSecondTapJustTurnsShiftOff() {
        assertEquals(Shift.OFF, nextShift(Shift.ONCE, 1_000))
    }
}

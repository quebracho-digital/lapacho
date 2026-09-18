package digital.quebracho.lapacho

import digital.quebracho.lapacho.storage.Sensitivity
import uniffi.lapacho_mobile_bridge.classifySensitivity

/** lapacho-core's classifier (Rust, the same one desktop runs) on the Kotlin enum. */
fun classify(text: String): Sensitivity = Sensitivity.valueOf(classifySensitivity(text).uppercase())

/** Credentials and secrets: the IME never stores them, and nothing shows them. */
fun Sensitivity.isSecret(): Boolean = this == Sensitivity.CREDENTIAL || this == Sensitivity.SECRET

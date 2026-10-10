# Audio capabilities

Tidegauge uses public CoreAudio properties. It neither installs an audio driver nor creates a process tap, records audio, or requests microphone/screen-recording access.

## Output controls

The default **media** output is `kAudioHardwarePropertyDefaultOutputDevice` (`dOut`), not the separate system-alert output (`sOut`). Volume and mute refer to that physical/virtual output device's output scope and main element.

Controls require a readable property and a successful `AudioObjectIsPropertySettable` capability check. Volume must be finite and within 0–1. Every write rechecks the default device identity and capability; changing outputs while the dashboard is open must not redirect an old click to a new device. Native failures are shown rather than treated as success.

Some outputs do not expose a master-volume or mute property. Tidegauge does not fake support, write unrelated channels, or partially change a stereo balance. Use macOS Sound settings when the corresponding control is unavailable.

## Per-process limits

The process-object list and `kAudioProcessPropertyIsRunningInput` / `IsRunningOutput` describe I/O activity, not a signal level or volume. Process activity APIs require compatible macOS versions (process objects were introduced in macOS 14.2); an empty result on an older system does not establish that all apps are silent.

`kAudioDevicePropertyProcessMute` (`appm`) applies to the **calling process** on a real device, not to an arbitrary process object/PID. It cannot implement another app's mute button. `kAudioHardwarePropertyProcessIsAudible` (`pmut`) likewise concerns the caller. The earlier unused setter and indicative-level path have been removed.

Arbitrary per-app volume/mute needs a separately designed routing solution, permissions, and verification that only the intended output changes. That remains open; unsupported controls are not wired to an unrelated API. No synthetic level meter is displayed.

## Native references

The installed SDK's `CoreAudio.framework/Headers/AudioHardware.h` documents the property semantics; `AudioHardwareBase.h` defines the object-property interfaces. Apple references:

- [Default output device](https://developer.apple.com/documentation/coreaudio/kaudiohardwarepropertydefaultoutputdevice)
- [Property capability check](https://developer.apple.com/documentation/coreaudio/audioobjectispropertysettable(_:_:_:))
- [Process mute](https://developer.apple.com/documentation/coreaudio/kaudiodevicepropertyprocessmute)
- [Process output activity](https://developer.apple.com/documentation/coreaudio/kaudioprocesspropertyisrunningoutput)

Unit/capability probes and VM rendering do not establish audible changes on every physical output. Verify supported volume/mute controls using an owned audio source on the target Mac; restore its previous state afterward. Never infer successful audio control solely from a button click.

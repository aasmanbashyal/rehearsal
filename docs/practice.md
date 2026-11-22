# Practice with Rehearsal

[Back to Rehearsal](../README.md)

## Cue cards and the recorder

In **Settings → Cue cards**, choose a TXT file with one speaking topic per line, or a CSV with a `topic` or `prompt` column. A single-column CSV also works without a header. Put quotes around CSV fields that contain commas. After import, choose **Practice my topics** to open the deck, or select **My topics** in the cue card category picker. Imported topics have no assigned difficulty. You can set the preparation time and speaking target in the same settings section.

Filter the 90 cue cards by category or difficulty, or choose **Shuffle**. Set your preparation time in seconds and press **Start**. When the countdown ends, Recorder opens and starts a take with that cue card and the selected recording mode. For screen modes, choose what to share before the countdown. **Reset** cancels the countdown. To skip preparation and open the controls yourself, choose **Record this topic**. The speaking target is a guide. You choose when to stop.

Choose **Voice**, **Camera**, **Screen**, or **Screen + camera**. Camera modes show a live preview before you record. The preview and the saved video have the same orientation. For screen capture, choose a screen or window from the sharing dialog. During **Screen + camera** recording, the preview shows the shared screen with your camera in the lower-right corner. Press `Alt+R` to start or stop a take while Recorder is open.

Browser mode and the Windows full-media build can replace the camera background with a solid color using the local [MediaPipe Selfie Segmenter](https://developers.google.com/edge/mediapipe/solutions/vision/image_segmenter).

## Sessions

In **Sessions**, choose **Select** to enter selection mode. Check individual takes or choose **Select all**, then **Delete selected**. Selection applies to the filtered list or the selected calendar day. Choose **Done** or press Escape to return to browsing. Changing the filter, view, or day also exits selection mode. Deletion asks for confirmation; takes that could not be deleted remain selected for retry.

## Teleprompter and script review

Choose **Add script** or **Edit script**, then **Save script**. Adjust the pace and text size, mirror the text, or go full screen. The teleprompter has its own recording mode and starts in Voice mode.

After transcription, open the take and select **Practice this take**. Listen to missing passages, different words, and extra words. Mark each item as practiced or dismiss it. **Show full comparison** puts the differences back in context. Listen to the recording before treating a transcription difference as a speaking mistake.

## Transcription and feedback

The recommended Parakeet 110M model is 129 MiB. **Settings → Transcription models** also lists larger Parakeet and Whisper options. Parakeet can provide word timings for script review; Whisper timestamps each transcript line. To retry with the selected model, open a saved take's action menu and choose **Transcribe again**.

The first model download also installs the [Handy transcribe.cpp](https://github.com/handy-computer/transcribe.cpp) runtime. Rehearsal verifies both downloads against upstream SHA-256 hashes. Automatic setup supports Windows x64, Linux x64 or arm64, and macOS x64 or arm64 through the shared Rust backend. **Refresh model list** checks the [Handy catalog](https://github.com/cjpais/Handy/blob/main/src-tauri/src/catalog/catalog.json) when the local copy is more than a day old.

Open **Speaking feedback** on a transcribed take. The default prompt asks for a strength, one change with a spoken rewrite, and a drill for the next take. Select a timestamp in the feedback to play that moment. **Transcript stats** shows word count, approximate pace, and fillers.

Expand **Edit prompt for this take** to change the instructions, then fold the editor closed to keep reading. **Copy prompt** includes the instructions and timestamped transcript. To receive feedback directly, choose Ollama or OpenAI in **Settings → Feedback**. You can also save a default coaching prompt there. Each take keeps its latest review and the instructions used to generate it. **Practice again** opens the recorder or teleprompter for another attempt.

For OpenAI, enter your key once and choose **Save feedback settings**. Rehearsal saves it in Windows Credential Manager, macOS Keychain, or your Linux Secret Service keyring. After restarting, Settings shows a masked placeholder and the saved status. Enter a new key to replace it, or choose **Remove saved key**. Switching providers keeps the saved key. The backend also accepts `OPENAI_API_KEY` when there is no saved key or the credential store is unavailable.

Recordings are written to disk in chunks during capture. If writing fails, capture stops and **Retry save** keeps the remaining data available while the page stays open. After an app or browser restart, look under **Interrupted recordings** on the Recorder page. **Recover take** saves the available portion as a normal session. If recovery cannot read the partial media, **Download saved portion** preserves those bytes for external repair. **Discard** asks before deleting them. The browser requests chunks once a second, but may delay delivery; audio or video that has not reached disk can still be lost in a crash. Recovery does not guarantee a playable file for every interrupted codec or device.

## Storage and backups

| Mode | Default data folder |
| --- | --- |
| Windows | `%LOCALAPPDATA%\Rehearse` |
| Linux | `${XDG_DATA_HOME:-$HOME/.local/share}/rehearse` |
| macOS | `~/Library/Application Support/Rehearse` |
| Browser | `.cadence` in the checkout |

Back up your recording folders to preserve takes, scripts, and feedback. The latest script draft, appearance, and per-take practice marks live in the browser or webview profile.

Keep your original topic lists or back up `custom-topics.json` alongside the app's storage settings. Reimporting the same topics reconnects the cue cards to their recorded takes.

In **Settings → Storage → Practice backup**, choose **Export practice backup** to download the current script draft, appearance, preferences, and practice marks as a JSON file. This file excludes API keys, recordings, transcripts, and recording-folder or deletion settings. Keep it alongside a backup of your recording folders.

Choose **Restore practice backup** to select that file. Rehearsal validates it and asks before replacing the saved draft, preferences, and practice marks. Recordings remain unchanged. Practice marks refer to the original session IDs, so restore the matching recording folders as well when moving to another computer.

Each take can be up to 15 minutes and 160 MiB. **Settings → Storage** lets you choose where new takes go, reduce their bitrate, and set a retention limit. Earlier takes stay in their original folders. If you set a limit, saving a new take permanently deletes the oldest takes above that limit. Browser mode and the Windows full-media build can also recompress imports.

<p align="center"><img src="public/assets/mark.svg" width="56" height="56" alt=""></p>

# Rehearsal

Practice spoken English by recording a short answer, listening for one mistake, and trying the sentence again. Rehearsal keeps your takes on your computer so you can hear what changed between attempts.

[Install Rehearsal](#install) · [Watch the demo](#watch-the-demo) · [Try a practice session](#try-a-practice-session) · [Help and reference](#help-and-reference) · [Contributing](#contributing)

## Try a practice session

1. **Pick something to say.** Open **Cue cards** and choose a topic such as *A project*. Give yourself a minute to describe what you did. If you have a talk written out, paste it into **Teleprompter** instead.
2. **Record your first take.** In **Recorder**, choose **Voice** and select **Start recording**. Speak naturally, then select **Stop and save**. For a presentation, you can also record your camera, your slides, or both together.
3. **Find one thing to fix.** Click a line in the transcript to hear your exact words. For example, you might hear “Yesterday I explain the project” and decide to use the past tense. **Speaking feedback** can suggest a rewrite. For a scripted take, **Practice this take** highlights words you changed, added, or skipped.
4. **Say it again.** Choose **Record again** and try “Yesterday I explained the project.” Open both takes from **Sessions** and listen to that sentence in each.

For transcripts, install the recommended **Parakeet 110M** model (129 MiB) from **Settings → Transcription**. It runs locally after setup. You can record and listen back before installing a model.

## Watch the demo

![Rehearsal’s review screen with playback controls and a timestamped transcript for the bundled example take.](docs/media/rehearsal-review.png)

If you’re running Rehearsal in browser mode, [open this example take](http://127.0.0.1:4317/#review/example) and select any transcript line to hear it.

![One-minute Rehearsal tour showing cue cards, a voice take, slide recording, transcript replay, and the sessions calendar.](docs/media/rehearsal-tour.gif)

The silent tour loops here. At **0:10** a cue card starts a recording, at **0:28** a transcript line plays the speaker’s words, and at **0:41** the calendar brings earlier takes back into view. [Watch the full tour with sound](https://cdn.jsdelivr.net/gh/aasmanbashyal/rehearsal@v0.3.0/docs/media/rehearsal-demo.mp4)

## Install

| Platform | Get started |
| --- | --- |
| Windows 11 x64 | [Run the installer](https://github.com/aasmanbashyal/rehearsal/releases/download/v0.3.0/Rehearsal-Setup-windows-x64.exe), then open **Rehearsal** from the Start menu. A [standalone ZIP](https://github.com/aasmanbashyal/rehearsal/releases/download/v0.3.0/Rehearsal-windows-x64-standalone.zip) is also available. |
| Ubuntu 24.04 x64 | [Download the Debian package](https://github.com/aasmanbashyal/rehearsal/releases/download/v0.3.0/rehearsal_0.3.0-1_amd64.deb), then run `sudo apt install ./rehearsal_0.3.0-1_amd64.deb` in the download folder. |
| macOS 12+ with Safari 16.2+ | Build `Rehearsal.app` with `./scripts/build-macos.sh` (see [build from source](docs/build.md#macos-12)), then drag **Rehearsal** from the DMG to Applications. Apple silicon and Intel are supported. |
| Run from source | Install the [build prerequisites](docs/build.md#browser-mode), run `bun start`, and open [the local app](http://127.0.0.1:4317/). |

Earlier releases used the name **ArticuPath**. [Release notes](https://github.com/aasmanbashyal/rehearsal/releases)

## Help and reference

| Term | What it means in Rehearsal |
| --- | --- |
| **Cue card** | A speaking prompt with points you can cover. Choose from 90 built-in cards or import your own topics from a TXT or CSV file. |
| **Take** | One saved recording. Use **Sessions** to tag it, find it by date, or import an existing recording. |
| **Transcript** | The speech model’s written version of a take. Select a timestamped line to listen, or export the text or SRT subtitles. |
| **Script difference** | A word or passage that differs between your script and the transcript. Replay the line to check whether you said it differently or the model heard it incorrectly. |
| **Filler** | An expression such as “um” or “you know.” **Speaking feedback → Transcript stats** shows detected fillers, word count, and approximate words per minute. |

**How do I get feedback?** Open a transcribed take and expand **Speaking feedback**. Use **Copy prompt** with an assistant of your choice, or configure Ollama or OpenAI in **Settings → Feedback**. The default prompt asks for one strength, a suggested rewrite, and a short drill for your next take.

**Where do my recordings go?** Recordings and transcripts stay on your computer. Ollama can review the transcript locally. If you choose OpenAI and request feedback, Rehearsal sends the coaching prompt and transcript text to OpenAI. Saved OpenAI keys use the operating system’s credential store.

**Where can I learn more?** The [practice guide](docs/practice.md) covers timers, script review, and recording limits. See [storage and backups](docs/practice.md#storage-and-backups) to move your work, or [open an issue](https://github.com/aasmanbashyal/rehearsal/issues) to report a problem.

## Contributing

Bug fixes, documentation edits, and improvements to the practice workflow are welcome. If you find a bug, include your operating system, whether you used the desktop app or a browser, the steps to reproduce it, and what you expected to happen. [Search or open an issue](https://github.com/aasmanbashyal/rehearsal/issues)

1. Fork and clone Rehearsal, then create a branch for your change.
2. Install Node.js for the browser checks and follow the [build prerequisites](docs/build.md#browser-mode) for your platform. From the project folder, run `bun install --frozen-lockfile` and `bun start`. Open [Rehearsal locally](http://127.0.0.1:4317/).
3. Run the checks below and try the workflow your change touches.
4. Open a pull request with a short summary, the checks you ran, and a screenshot or short recording for visible changes. Use the bundled example or other test data in anything you share.

### Run the checks

```sh
bun test ./tests
bunx playwright install chromium
bun run test:ui
```

The browser checks start their own app instance and save screenshots and `measurements.json` under `.cadence/ui-check-*`. They use synthetic input for the microphone, camera, and screen. See the [frontend guide](docs/frontend.md#verification) for Windows WebView2 checks and other browser options.

For a quick manual check, open **Cue cards**, pick a topic, and select **Record this topic**. Record a short **Voice** take, select **Stop and save**, then play it back and confirm it appears in **Sessions**. You can do this before installing a transcription model.

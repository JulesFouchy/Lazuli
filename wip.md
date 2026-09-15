## Triage


transparent images background, eg the transition isf demo in coollab timeline, in the viewer you see the timeline below it as it is not 100% opaque

when we copy paste an image file it should work to import the image

we should see the current date at the top

hide top bar, like in zen (also make it a custom titlebar with our own style)
F11 should toggle fullscreen

the button shouldn't say "Open folder..." but "Open project...", or "Import project..."? (and on hover we explain that each project is just a plain local folder)

setup auto-updates
make the proper installer (what do we need to setup the app ? are there some info i need to fill in ? I want to do everything properly)

bug with the custom color selector it always closes when we start dragging

- timeline entry with the new style + name + icon
- make a wide image for a banner, in the same style as the icon, like an extended version of the icon
- make Dark theme the default theme, not system
- run powershell -ExecutionPolicy Bypass -File "C:\Users\fouch\AppData\Local\Temp\claude\c--Dev-journaley\1aff154b-031d-489e-996b-601aa60984d4\scratchpad\finish-rename.ps1"
- consider here is the image to use as the icon : "C:\Users\fouch\Downloads\ChatGPT Image Sep 15, 2026, 10_13_29 PM.png"
- The rename goes all the way down — crate, lapis_lib, bundle id dev.lapis.app, product name, window title, localStorage keys, and the project marker file, now lapis.yaml. The four in projects/ are renamed. I deliberately did not add a journaley.yaml fallback: nothing on disk anywhere needed one, and a compatibility path would have outlived its last user by years. Say the word if you'd rather have it.

The bundle-id change moved %APPDATA%\dev.journaley.app → dev.lapis.app. I copied your settings across; the old folder is untouched, delete it when you're happy. Git remote repointed and verified. The memory note on driving the app over CDP was full of journaley.exe / dev.journaley.app — updated and renamed.

Theme. Dark ground is #0b1020 lapis, accent is gold #e3b04a. The surface ramp is scaled from the ground rather than mixed toward white, so a raised card stays the same hue instead of going grey. Verified in the running app — your lapis-crystal cover on the Lapis project sits with it beautifully.

Light theme: I kept it, and made it the other half of the same image. Warm vellum paper stays; what changed is that the ink went blue-black (#141a2c) instead of neutral. So dark is gold on stone, light is ink on vellum — one palette, two lights, rather than two unrelated themes. That felt much better than dropping light mode or making it a pale blue.

Background is now overridable, per theme, alongside the accent, with presets (Lapis / Ink / Bistre / Verdigris / Porphyry on dark; Vellum / Paper / Chalk / Linen on light) and a colour well. It reaches the window before first paint through both existing paths — the boot script and settings.json.

One deliberate limit: the override derives only the surfaces, not text and lines. Letting it compute the ink too would make every override a chance to produce an unreadable page. The cost is that a wildly out-of-range ground (a pale blue while in dark theme) will look wrong. Tell me if you'd rather it be fully self-correcting — that's a real piece of work, not a tweak.

Icon — a lapis tile with a vein of gold and pyrite flecks, in src-tauri/icons/. Drawn by scripts/make-icon.mjs straight to PNG, procedurally: rasterising an SVG needs a browser or a native library, and both headless renderers on this machine screenshot before they paint. Still reads at 32px as a blue tile with a gold streak.

Launch screen lost the title and tagline as asked.

README has a "Why Lapis" section with all six of your reasons, plus the two you didn't mention: lapis is Indonesian for layer, and Latin for stone.

Tagline
I put "One picture a day, until the project is done." in the README — it's concrete and says what the discipline actually is. Yours were all about pictures telling something, which is true but a bit soft. Alternatives if you want a different angle:

A long project, laid down one day at a time. (leans on the layer meaning)
Every day you worked leaves a mark.
A picture and a sentence for every day you worked on it. (the old in-app one — the most literal)
What a long project looked like, day by day.
Two things I fixed on the way
The README claimed dropping an ffmpeg.exe into src-tauri/binaries/ would bundle it. There's no externalBin or resources entry in tauri.conf.json — that never worked, which is presumably why you rewrote those error messages. And 256x256.png was still the old Journaley icon: tauri icon writes that size as 128x128@2x.png and leaves the other name alone. That's the second commit.

One departure from the commit convention
Your tauri.conf.json bundle block rode along in my commit. Trimming the generated icon set down to the Windows sizes only makes sense against your targets: ["nsis"], so the two genuinely depend on each other. Your commands.rs and export-dialog.ts ffmpeg-message edits are still sitting uncommitted and untouched, and nothing was left staged.

## Import initial projects

projects:
- Personal

For Coollab tell it to look through the git history to fill in the timeline (but what about images ? Maybe i just accept it starts from september, and i just fill in the first days of septembre manually, with end of intersnhsip, communale, my holliday break)
Add soirée test communaele + screen of wip web ui + conf lgm, ui investigation + first and last day of all three interns + brussel lots of ideas for mapping, but also automation graph, control flow, spreadsheet data for entities simulation etc 

And don't put all the code details for the past ones, i can't be bothered getting images for all of them. Just big milestones like UI prototype, ISF implementation, test framework, IrGraph/compiler, 3D renderer, text

For Coollab images, never show code, always a render or the UI, because it's more visual, and eg we can see the ui evolving over time
Maybe find some old images on discord ?

Store the Coollab project on the Coollab repo so other collaborators can add entries too

## Cross-project timeline

So I can see all the things I did in one place, when I was more on a given project, etc

## Use Coollab to produce a small standalone app that exports the videos for journaley ?

That would be very cool, and allow for previewing in real time. It would require:
- the spreadsheet/table thing to get the list of entries from the folder, parse them, and create the table
- a lua script to create the custom node that reads an entry file and returns the Entity/Struct
- a way to Loop over all the files in the folder (maybe the Map that takes the folder and produces the Table)
- probably some improvements to text renderer
- export as standalone exe (or at least coollab player, honestly standalone exe is not that useful except for like submitting to a demoparty, otherwise just installing the payer once is much better, and then all coollab "apps" ie project files can just be openend in the player and executed as is, hile still being able to be open in the editor to change them or inspect them)
- ideally we need to integrate it into journaley, so it requires the standalone exe to have a cli to drive it, export a given frame, at a given resolution, etc. All these should probably be normal graph inputs, that we expose as cli args when exporting to exe.
  - but for now we can just have a coollab project, and do the export entirely from coollab
- the event graph needs to be able to export a video. 
- how do we chain all images one after the other ? create a timelnie ? but the graph has to create it programmatically from the list of entries
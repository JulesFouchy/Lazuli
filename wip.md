## Triage

transparent images background, eg the transition isf demo in coollab timeline, in the viewer you see the timeline below it as it is not 100% opaque

when we copy paste an image file it should work to import the image

tagline ideas : "tell your story/journey through pictures"? "Let pictures tell your journey"? "Pictures that tell your journey".

we should see the current date at the top

hide top bar, like in zen (also make it a custom titlebar with our own style)

the button shouldn't say "Open folder..." but "Open project...", or "Import project..."? (and on hover we explain that each project is just a plain local folder)

bug with the custom color selector it always closes when we start dragging

- consider using this as the icon : "C:\Users\fouch\Downloads\ChatGPT Image Sep 15, 2026, 10_13_29 PM.png"

- the "could not open" popup doesn't stay long enough on screen i couldn't read
- "open folder" button shouldn't open the project page


- release version 1.0
- setup store to sell for 1€

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
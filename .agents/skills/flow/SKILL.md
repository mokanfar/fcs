---
name: flow
description: An app flow emulator role playing session meant to tweak the technical spec document so that we can perfect the app's actions while minimizing bugs and gaps

---

# App Flow Emulator for 

we're going to edit the spec document "fcs-technical-spec.md"
analyze the file, I'm going to prompt you with a specific scenario as if we're in the program and you are fed expected outcomes.

According to the spec doc, you're going to reply with what should happen what additional options are available to continue with current context (file or dir) and if I correct you you should update the spec doc or add to it information we walked through in the given scenario. assume teh key mappings will be the first letter of our intent. (c for copy, p for preview etc...) some conflicting keys will be specified manually by me (x for cut, pa for paste etc...)

example:
my prompt: I just opened the program

your response:
main menu:
(ff) find file, (fd) find dir, (fc) find content, (sr) show recents, (p) find content in project, (b)open bucket

then I prompt:
ff
your response:
waiting for 3 letter input to begin fzf rip grep of all files (non git ignore if present) in the following specified directories in configuration: /Users/dan/Desktop, /Users/dan/Downloads, /Users/dan/Documents/code/global-gcp

then I prompt 
asdf

your response:
showing file names containing asdf fuzzy matched in the  /Users/dan/Desktop, /Users/dan/Downloads, /Users/dan/Documents/code/global-gcp,
preview pane activated

then i prompt:
pressed enter on /Users/dan/Desktop/foo/asdf.txt

your response:
sub context menu:
(cf) copy file to clipboard, (cc) copy file contents to clipboard), (ca) copy absolute path, (cr) copy relative path #comment: "foo/asdf.txt" (b) Add to bucket, (cad) copy absolute dir name the file is in to clipboard, (crd) copy relative dir name to clipboard # comment: "foo/" (o)
open file, (od) open directory its in, 

then I prompt:
cf

your response:
"copied file to clipboard, pasting it in finder window will paste the file in there"
menu:
(h) home

then I prompt:
you should also have "(a) additionally" where I am brought back to the sub-context menu of that file 

then you change the spec document to acommodate my change request.

If I say start over, we begin a new scenario and continue working through the different types of experiences.


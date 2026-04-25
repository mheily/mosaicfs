 Yes — the cross-phase acceptance checklist from the design notes. Working through it in order:                        
                                                                                                                        
  1. Relaunch the app without re-authorizing — click the same file again. It should open directly without prompting     
  (bookmark persisted across launches).                                                                                 
  2. Unmount the watch directory (or rename it temporarily so the path is unreachable), click a file → expect "File not 
  reachable" flash with a Retry button.                                                                                 
  3. Re-mount/rename it back, click Retry → file opens.                                                                 
  4. Symlink escape test — inside the dev data dir, create a symlink pointing outside it: ln -s /etc/hosts              
  /Users/robot/mosaicfs/dev-data/outside-link.txt, then index it and click it in the browser. Expect "Refusing to open …
   a symlink resolved to /etc/hosts" flash with no button. Clean up the symlink after.                                  
  5. Delete the network mount row for this node in the admin UI (Settings → Nodes → this node), then click a file from  
  that node → expect "No mount configured for node X on this host" flash with no button and no picker appearing.        
  6. Browse to http://localhost:8443/ui/browse directly in Safari (not through the app), click a file → expect "This
  file can only be opened from the MosaicFS desktop app." flash.                                                        
  7. Wrong-directory picker — trigger authorization, but when NSOpenPanel opens navigate to a different directory than
  requested and click OK → expect "You selected {got}, but MosaicFS needs permission for {expected}" flash with a "Try  
  again" button. Clicking it re-opens the picker.                 
                                                                                                                        
  Steps 2–3 are the most likely to surface edge cases you haven't hit yet. Steps 4 and 7 are the security-relevant paths
   worth confirming work.

====
RESULT:

1. Works
2 and 3. Fail. After renaming it, the expected message did not appear. It instead asked me to re-authorize the directory, and then the system dialog asked me to authorize the parent of the directory I renamed away.
   Another fun fact: the indexer saw the files were missing, so it removed them from the index(!) They came back
   after the directory was renamed back to what it was. After all that, the permissions bookmark worked again.
4. Could not test. The file is never added to the index. I waited a minute, and it never showed up. 
   This is by design in the code; symlinks are not followed by the indexer.
5. Works.
6. Works
7. Works. I picked the parent directory, and got the error: "You selected /Users/robot/mosaicfs, but MosaicFS needs permission for /Users/robot/mosaicfs/dev-data. "

(To resume testing, use session name "claude011")

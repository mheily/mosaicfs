# Requirements for a MosaicFS file browser

This document contains a list of behavioral requirements for the web-based file browser.

These are divided into phases based on the level of importance.

## Phase 1

### Backend

* The mosaicfs-server uses the /browse route to serve the file browser; this is different from
  the existing /admin/browse route that is for administering and configuring the filesystem.
* Use HTMX to make the app dynamic as needed; do not pull in any large JavaScript frameworks.

#### API Routes
* `GET /browse`: Serve the main file browser page.
* `GET /browse/list`: Return the paginated and optionally filtered list of directory entries for a given path.
* `PUT /browse/open`: Request the server to open a file using the system's default application.
* `GET /browse/navigate`: Return the updated page content (toolbar and file listing) when changing directories.

### Launcher

* The file browser is intended to be opened as an "app" with no toolbars or address bar.
	- Example: google-chrome-stable --new-window --app=http://localhost:8443/browse

### Main toolbar

The top area of the page is a toolbar with the following items arranged from left to right:

1. Back button. This is greyed out initially.
2. Forward button. This is greyed out initially.
3. Location bar. This shows the full path that is being browsed.
4. Search bar. This searches the current directory

The behavior of items in the main toolbar is:

* The browser back button goes back to the previous directory via the built-in browser history.
* The browser forward button goes to the next directory via the built-in browser history.
* When the user clicks anywhere inthe location bar, the entire text is highlighted by default.
They may edit the location and press Enter to proceed. This browser only browses MosaicFS; 
the "/" path represents the top level directory of the virtual filesystem. Special entries 
(e.g., ~, $HOME, ., ..) are not supported.
When the user presses Enter, the window will navigate to the new location. If that location
does not exist, or if an error occurs (e.g., permission denied), the error is displayed
using the flash() mechanism underneath the toolbar, and the previous content of the
location bar is restored.
* When the user types in the search bar, the list of files is filtered to match the search.
    This is done case-insensitively via server-side requests. Because the file browser uses 
    pagination to fetch results as the user scrolls, performing a search invalidates the 
    current set of rows and resets the view to a fresh set of filtered pages.

### File listing

Beneath the main toolbar is the file listing, which is organized into rows and columns.

Behavior:
* Hovering over a file underlines the name of the file, indicating that it is clickable.
* A single left click on a file opens the file in a new window.
    This is achieved by calling `PUT /browse/open?path=<full_path>`. The server, running with 
    the user's credentials and desktop environment access, opens the file using the 
    system's default mechanism (e.g., `/usr/bin/open` on macOS or `/usr/bin/xdg-open` on Linux) 
    via `std::process::Command`.
* A single left on a directory opens the directory in the same window. 
* When sorting rows by name, ignore case. Folders are shown first in the list, followed by regular files.
* If a directory has no files, the message "This directory is empty" is displayed instead of the file listing.
* When sorting by date, use modification date (mtime).
* Sizes are displayed using B, K, M, G as suffixes for byte, kibibyte (1024B), mibibyte (1024K), gibibyte (1024M).
	- Avoid floating point numbers by rounding up to the next whole number.
        - When the value is larger than 999, move up to the next unit.
	
	Examples of how to convert from SI units to Mosaicfs display file sizes:
		- 0 bytes -> 0
		- 885 bytes -> 1K
		- 1024 KiB -> 1M
		- 1524 KiB -> 2M
		- 999 MiB -> 999M
		- 1000 MiB -> 1G

* The list of directory entries has a heading row with Name, Size, and Date. Clicking on a cell in this row
causes the entire list to be sorted by that column, in ascending mode. Clicking on the same cell a second
time will change the sorting from ascending to descending.

Here is a rough ASCII art rendering of the file browser:

+----------------------------------------------------------------+
| <Back> <Forward> <Location bar> <Search box> |
| 
| Name | Size | Date |
| --------------------
| foo  | 12M  | 2026-01-01 |
+----------------------------------------------------------------+

## Phase 2

These items are deferred until phase 2:

* Sidebar
* Keyboard navigation support
* Searching all subdirectories

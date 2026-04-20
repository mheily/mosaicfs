# Requirements for a MosaicFS file browser

This document contains a list of behavioral requirements for the web-based file browser.

These are divided into phases based on the level of importance.

## Phase 1

### Backend

* The mosaicfs-server uses the /ui/browse route to serve the file browser.
  Prior to this change, that same URL served an administrative directory-browsing
  page; change 010 replaces its content with the end-user file browser. VFS directory
  configuration (create / edit / delete) lives at /ui/vfs/* and is unchanged.
* Use HTMX to make the app dynamic as needed; do not pull in any large JavaScript frameworks.
* Authentication: /ui/browse and all /ui/browse/* endpoints require the same session cookie
  used by the rest of /ui. Unauthenticated users are redirected to /ui/login. This matters
  because POST /ui/browse/open spawns OS processes with the server user's credentials.
* Pagination: /ui/browse/list returns 50 entries per page. The client fetches the next page
  via HTMX when the last visible row scrolls into view.

#### API Routes
* `GET /ui/browse`: Serve the main file browser page.
* `GET /ui/browse/list`: Return the paginated and optionally filtered list of directory entries for a given path.
* `POST /ui/browse/open`: Request the server to open a file using the system's default application.
* `GET /ui/browse/navigate`: Return the updated page content (toolbar and file listing) when changing directories.

### Launcher

* The file browser is intended to be opened as an "app" with no toolbars or address bar.
	- Example: google-chrome-stable --new-window --app=http://localhost:8443/ui/browse

### Main toolbar

The top area of the page is a toolbar with the following items arranged from left to right:

1. Back button. Greyed out on initial page load.
2. Forward button. Greyed out on initial page load.
3. Location bar. This shows the full path that is being browsed.
4. Search bar. This searches the current directory

The behavior of items in the main toolbar is:

* The back button goes back to the previous directory via the built-in browser history.
* The forward button goes to the next directory via the built-in browser history.
* After the user performs any navigation, both buttons remain enabled for the rest of the
  session. Browser history depth is not reliably introspectable from JavaScript, so the
  buttons cannot be dynamically re-disabled when the user reaches the start or end of the
  history stack; clicking when there is nothing to go to is a no-op.
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
    current set of rows and resets the view to a fresh set of filtered pages. Changing the
    sort column or direction resets pagination for the same reason.
* Search term and sort state reset to their defaults (empty search, sort by name ascending)
  whenever the user navigates into a different directory.

### File listing

Beneath the main toolbar is the file listing, which is organized into rows and columns.

Behavior:
* The default sort on first page load is by name, ascending.
* Every entry in the virtual directory is shown; there is no hidden-file convention in the
  MosaicFS namespace.
* Hovering over a file underlines the name of the file, indicating that it is clickable.
* A single left click on a file asks the server to open the file using the OS's default
  application. The resulting window belongs to that application, not the web browser.
    This is achieved by calling `POST /ui/browse/open?path=<full_path>`. The server, running with 
    the user's credentials and desktop environment access, opens the file using the 
    system's default mechanism (e.g., `/usr/bin/open` on macOS or `/usr/bin/xdg-open` on Linux) 
    via `std::process::Command`.
    On success, no flash message is shown (the user gets feedback from the application
    window appearing). On failure — file not found, path not accessible on this node, or the
    opener command exited with an error — the error is displayed in the flash region below
    the toolbar.
* A single left click on a directory opens the directory in the same window. 
* When sorting rows by name, ignore case. Folders are shown first in the list, followed by regular files.
* If a directory has no files, the message "This directory is empty" is displayed instead of the file listing.
* When sorting by date, use modification date (mtime).
* Dates are displayed in ISO format (YYYY-MM-DD) in the server's local timezone.
* Sizes are displayed using K, M, G as suffixes for kibibyte (1024 bytes), mibibyte (1024 KiB),
  gibibyte (1024 MiB). The B unit is not shown as a displayed value (see rule below).
    - Rule: exactly 0 bytes displays as `0` (no unit). Any non-zero size strictly less than
      1024 bytes displays as `1K`. For sizes ≥ 1024 bytes: repeatedly divide by 1024 and
      ceil, promoting units (K → M → G) until the integer result is ≤ 999.
    - Never display floating-point values; always round up to the next whole number.

	Examples of how to convert from raw byte counts to MosaicFS display file sizes:
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

# 0032 — The operating system's own file dialogs

**Status:** decided and built, 2026-11-18 · **Milestone:** 0.10

## Question

The app chose files in `egui-file-dialog` 0.15, a dialog drawn inside egui.
It followed the theme and never held up the window, but it listed a folder by
name only and could not sort. In a Downloads folder of a few hundred files, a
STEP file just downloaded could not be found. Sorting has been
asked of it upstream since 2024. Explorer's dialog, the desktop's file chooser
on Linux and the panels on macOS sort by date, search, and list recent files.
Which dialog should the app use?

## What was decided

- **The operating system's own, through `rfd` 0.18** (MIT): Explorer's
  `IFileDialog` on Windows, the file chooser the XDG desktop portal shows on
  Linux, `NSOpenPanel` and `NSSavePanel` on macOS (`file_dialog.rs`).
- **The window is never held up.** A click records what it wants. The next
  frame builds an `rfd::AsyncFileDialog` on the window's thread, as macOS
  needs, with the window as its parent so the dialog is modal to it. A thread
  waits for its answer (`pollster`, which eframe already brings) and wakes the
  window when it comes. On Windows and Linux rfd shows the dialog from a
  thread of its own; on macOS it is a sheet on the window.
- **One dialog at a time.** A request made while one is asked for or showing
  is dropped. With the dialog modal to the window, that is only a shortcut
  pressed while it shows, or two requests in one frame.
- **Each kind of file keeps its own folder**: jobs, models, machine profiles,
  tool libraries, and the folder programs were posted into, kept between runs
  under the app's storage key `file_folders`. Models come from Downloads and
  jobs live elsewhere, so one folder for all would be wrong for one of them
  every time. Save As starts at the job's own file, and finding a missing
  model starts where it was. A folder that has gone is not passed, and the
  system chooses. rfd passes the folder to Windows with `SetFolder`, which
  opens there, not `SetDefaultFolder`, which Windows may overrule with the
  last folder it remembers. The old dialog's key, `file_dialog`, is no longer
  read or written.
- **Filters by extension, in both cases**: jobs, models (STEP, STP, STL),
  machine profiles (TOML) and tool libraries (CSV), then *All files*. Some
  CAD programs write `.STEP`. Windows and macOS match an extension in any case; a
  portal's chooser matches a pattern as written, so each extension is listed in
  lower and upper case. Save As offers jobs only: with *All files* chosen,
  macOS would add `.*` to the name.
- **`.camcraft` is added where the dialog does not add it.** Windows adds the
  chosen filter's extension to a name typed without one, macOS adds the first
  extension the panel allows, and both ask before replacing the file that
  names. A portal's chooser need not add it (GTK's does not), and asked only
  about the name it returned. So the app adds it, and if a file of the
  completed name is already there, asks before replacing it (*Replace
  rail-jig.camcraft?*). Saving no longer adds it by itself: a job opened from a
  file named otherwise is saved back to that file, not written beside it as
  `name.job.camcraft` over whatever that held.
- **rfd's features are `xdg-portal` and `wayland`**, named, with its defaults
  off. The portal backend loads `libdbus` as it runs, so nothing is built
  against on Linux, and the release still builds in WSL with no GTK development
  files. `wayland` lets the portal parent the dialog on a Wayland window. rfd
  is MIT; the one crate it brings that was not already built in,
  `objc2-uniform-type-identifiers` (macOS only), is Zlib, Apache-2.0 or MIT.

## What it costs

- **It is not drawn in the app's theme.** It is the system's, light or dark as
  the system is.
- **On Linux a dialog needs a file chooser portal**: `xdg-desktop-portal` with a
  backend for the desktop (`xdg-desktop-portal-gtk`, `-gnome` or `-kde`), or
  `zenity`, which rfd falls back to. With neither, no dialog appears.
- **A dialog that could not be shown looks like Cancel.** rfd 0.18 answers both
  with nothing, on every platform. On Windows every COM error is turned into no
  answer; on Linux a missing portal and a missing `zenity` are logged through
  the `log` crate, which the app does not read, and answered with nothing.
  Only what fails on the app's side is reported, on the job it was for: the
  waiting thread not starting, or stopping without an answer.
- **It cannot be pictured by the snapshot harness**, and no test can drive it.
  Under test no dialog is shown at all: the function that shows one is, in a
  test build, one that cancels it at once, so the harness, which drives the
  real app, cannot open one by any path. The `open_job_dialog` picture is gone.
  What is tested is everything around the dialog: the filters, the folders
  kept and forgotten, adding the extension and when to ask, and one dialog at a
  time.
- **On macOS an open panel shows one merged list of extensions**, not named
  filters: rfd gives the panel every filter's extensions at once. A save panel
  with one filter, as Save As has, allows that filter's extension only.

## What was considered and not chosen

- **A dialog of our own in egui.** Sorting by date is a day's work; searching,
  recent files, the system's places and network drives, thumbnails, and the
  keyboard habits of each system's dialog are not, and would still not be the
  dialog people know from every other program.
- **A fork of `egui-file-dialog` with sorting.** It would fix the one complaint
  and leave the rest, and a fork is a second project to keep up with egui.
- **rfd's GTK backends.** They need GTK's development files to build, which the
  Linux release's WSL build does not have, and GTK's dialog is what the GTK
  portal shows anyway.
- **rfd's blocking dialog on a thread.** On macOS a blocking panel runs a modal
  loop on the window's thread, and the window stops drawing behind it.

# Match the moved file when locating Trash metadata

Status: resolved
Type: bug
Priority: P1

Multiple Trash entries for one original pathname could make lookup return an older file whose metadata had a newer modification time. A native GIO fixture reproduced the wrong receipt. Lookup now filters by the source's captured filesystem identity before choosing a receipt. See `../receipt-red.log`, `../receipt-green.log`, and `../report.md`.

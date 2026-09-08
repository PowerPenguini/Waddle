# Reject delayed data-write failures before transfer publication

Status: resolved
Type: bug
Priority: P1

The copy engine never synchronized its output file before publication, so delayed ENOSPC/EIO could not be surfaced before replacing the destination or removing a Move source. Fixed by synchronizing the staged regular file after writing data and metadata. Eight fault-injection cases verify source/destination preservation and staging cleanup. See `../storage-red.log`, `../storage-green.log`, and `../report.md`.

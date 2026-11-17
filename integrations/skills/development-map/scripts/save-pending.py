"""Save one private handoff. Run from the worktree; list only assignment files."""
import argparse
import os
from pathlib import Path
import tempfile
from zipfile import ZipFile, ZIP_DEFLATED

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("directory", type=Path, help="private pending directory in the durable assignment workspace")
parser.add_argument("handoff", type=Path, help="exact request JSON or a note with node, generation, base and next action")
parser.add_argument("files", nargs="*", type=Path, help="changed/new files relative to worktree; list deletions in the note")
args = parser.parse_args()
root = Path.cwd().resolve()
if ".devmap-pending" not in args.directory.resolve().parts:
    parser.error("choose a session directory under .devmap-pending")
if args.handoff.resolve().parent != args.directory.resolve() or args.handoff.is_symlink():
    parser.error("move the original handoff into the private session directory first")
args.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
# Consumer repositories may not have installed a project-level ignore rule.
with (args.directory / ".gitignore").open("a+") as ignore:
    ignore.seek(0)
    if "*" not in ignore.read().splitlines():
        ignore.write("\n*\n")
with tempfile.NamedTemporaryFile(dir=args.directory, suffix=".partial", delete=False) as tmp:
    staged = Path(tmp.name)
try:
    with ZipFile(staged, "w", compression=ZIP_DEFLATED) as archive:
        archive.writestr("handoff", args.handoff.read_bytes())
        for path in args.files:
            if path.is_absolute() or ".." in path.parts or path.resolve() != root / path or not path.is_file():
                parser.error(f"expected a regular worktree file: {path}")
            archive.write(path, "files/" + path.as_posix())
    # Windows FlushFileBuffers requires a writable handle.
    with staged.open("r+b") as saved:
        os.fsync(saved.fileno())
    final = staged.with_suffix(".zip")
    os.link(staged, final)
    print(final)
finally:
    staged.unlink(missing_ok=True)

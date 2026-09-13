#!/usr/bin/env python3
"""Reproduce embedded verb data from a caller-supplied, pinned WordNet archive.
No download, extraction to disk, source mutation or user-state access.
"""
import hashlib
from pathlib import Path
import sys
import tarfile

archive, output = map(Path, sys.argv[1:])
expected = "640db279c949a88f61f851dd54ebbb22d003f8b90b85267042ef85a3781d3a52"
assert hashlib.sha256(archive.read_bytes()).hexdigest() == expected
with tarfile.open(archive, "r:gz") as source:
    license_text = source.extractfile("WordNet-3.0/COPYING").read().decode()
    verb_index = source.extractfile("WordNet-3.0/dict/index.verb").read().decode()
words = sorted({line.split()[0] for line in verb_index.splitlines()
                if line and not line[0].isspace()
                and line.split()[0].isascii() and line.split()[0].isalpha()})
assert len(words) == 8429
header = ("# Derived single-word verb lemmas from WordNet 3.0 index.verb.\n"
          "# Source: https://wordnetcode.princeton.edu/3.0/WordNet-3.0.tar.gz\n"
          f"# Archive SHA-256: {expected}\n")
text = header + "\n".join("# " + line for line in license_text.splitlines())
text += "\n\n" + "\n".join(words) + "\n"
output.write_text(text)
print(hashlib.sha256(text.encode()).hexdigest())

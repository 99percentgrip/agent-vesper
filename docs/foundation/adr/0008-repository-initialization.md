# ADR 0008: Repository Initialization

Status: APPROVED BY EXISTING REQUIREMENT

## Context

The target was not a Git repository and contained only existing DOX and
reconnaissance documents.

## Decision

Initialize Git with default branch `main`, preserve every file, configure no
remote, and create no commit.

## Consequences

Initialization completed locally. All existing files remain untracked until a
future explicitly authorized commit workflow.


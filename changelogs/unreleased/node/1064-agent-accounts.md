### Added

- Node: beep-node runs a session signed in as the named account it was
  started with, and keeps that account when the session resumes or the
  node restarts. Agents and reviewers that a session starts use the
  account of the same name.
- Node: beep-node refuses a start on an account it does not have yet.
- Node: beep-node reads the plan limits of each named account it holds,
  apart from the default account's.

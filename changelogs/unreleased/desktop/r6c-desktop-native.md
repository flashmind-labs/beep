### Changed
- Desktop: a message you send shows as a waiting row at once, instead of waiting up to a quarter second for the machine's first answer. One the machine refuses stays in the list with its reason.

### Fixed
- Desktop: a message's files are no longer removed from disk before the record that drops them is saved, so a crash cannot leave a message whose files are gone.

### Performance
- Desktop: model lists, folder listings, worktrees, session places, new-machine tickets and project sync details now arrive as pushes instead of repeated reads.

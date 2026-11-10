- `newgit merge` merges tracker by tracker when an instance changed only
  pins in root. That keeps working when root's history does not descend
  from the instance's, as after a roll-up that could not keep the merge's
  own commit.
- The remote sweeps each project within the hour of a landing. It rolls up
  what nobody has read yet and clears the landing refs that merges leave
  behind.
- A merge sends its commits to the remote in parallel.

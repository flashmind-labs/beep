### Performance

- Node: a project's branch list and each branch's upstream, ahead and
  behind counts are read directly from Git instead of starting a Git process
  for every branch whenever refs change. Branches refresh faster and use less
  battery.

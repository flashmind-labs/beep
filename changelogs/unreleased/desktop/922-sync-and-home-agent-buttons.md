### Changed

- Desktop: Fix with an agent on a file sync conflict and on a blocked Git
  sync now opens the harness and model picker under the button, like Fix
  this does. Picking a model starts the session right away. A sync conflict
  is fixed on the machine that has it, and a blocked Git sync starts with
  both checkouts and recent sessions in the prompt. It used to open a draft
  in the start form.
- Desktop: Review with agent on Home's pull request cards and in Home's pull
  request list also opens the picker and starts at once in the project's
  checkout. The review still waits for your approval before it posts
  anything. Reply on Home still opens the start form.

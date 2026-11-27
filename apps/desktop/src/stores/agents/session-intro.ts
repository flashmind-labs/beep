const sentences: readonly string[] = [
  "What are we building today?",
  "What should we make next?",
  "Got an idea worth trying?",
  "Where should we start today?",
  "What have you been thinking about?",
  "What would you like to create?",
  "What needs a fresh pair of eyes?",
  "Which idea should we try first?",
  "What can we bring to life?",
  "What would make today a good day?",
  "What is the next small step?",
  "What should we figure out together?",
  "What is waiting to be built?",
  "What would you love to finish?",
  "What is on your mind today?",
  "Which loose end should we pick up?",
  "What is worth a closer look?",
  "What should we take apart first?",
  "What could work a little better?",
  "What is the bug of the day?",
  "What is keeping that test red?",
  "What should this button do?",
  "What is the smallest useful version?",
  "What would you change about this?",
  "What should we sketch out first?",
  "Which rough edge needs some care?",
  "What could we make simpler?",
  "What is ready for another pass?",
  "What would you like to understand?",
  "What is hiding in that error?",
  "What should we try a different way?",
  "What is the interesting part?",
  "What would happen if we tried?",
  "What is missing from the picture?",
  "What is your next experiment?",
  "Which question should we chase?",
  "What have you been meaning to fix?",
  "What is almost working?",
  "What deserves a little polish?",
  "What should feel better to use?",
  "What would make this click?",
  "What is the first thing to test?",
  "What is the idea behind the idea?",
  "What is worth starting over?",
  "What could we learn by making it?",
  "What should we put on the screen?",
  "What would you like to automate?",
  "What is taking too many steps?",
  "What can we get working today?",
  "What is ready to leave the notebook?",
  "What should we turn into a prototype?",
  "What is the next piece of the puzzle?",
  "What would make this more useful?",
  "What are you curious about today?",
  "What should we follow through on?",
  "Which idea keeps coming back?",
  "What could use a second look?",
  "What is one thing we can improve?",
  "What should we explore together?",
  "What shall we build from here?"
];

let remaining: string[] = [];
let current = "";

// Deal every sentence once before reshuffling, including across cycle boundaries.
export function nextSessionIntro(): string {
  if (!remaining.length) {
    remaining = [...sentences];

    for (let i = remaining.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [remaining[i], remaining[j]] = [remaining[j], remaining[i]];
    }

    if (remaining.at(-1) === current) {
      [remaining[0], remaining[remaining.length - 1]] = [remaining.at(-1)!, remaining[0]];
    }
  }

  current = remaining.pop()!;
  return current;
}

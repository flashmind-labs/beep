//! Usernames such as `mega-fox-trot`, read from an iroh public key.
//!
//! A name is for people to read; the key stays the identity. About two
//! million names exist, so two keys can share one. Never reorder or edit
//! the lists: every name already handed out would change.

use iroh::EndpointId;

const ADJECTIVES: [&str; 128] = [
    "alpine", "amber", "atomic", "azure", "bold", "bouncy", "brave", "breezy", "bright", "brisk",
    "calm", "cheery", "clever", "coral", "cosmic", "cozy", "crimson", "crisp", "curly", "dandy",
    "daring", "deep", "dizzy", "dusty", "eager", "early", "easy", "electric", "epic", "fair",
    "fancy", "fast", "fierce", "fluffy", "frank", "fresh", "frosty", "funky", "fuzzy", "gentle",
    "giant", "glad", "glossy", "golden", "grand", "happy", "hardy", "hazy", "hidden", "humble",
    "hyper", "icy", "jazzy", "jolly", "jumpy", "keen", "kind", "lazy", "lively", "loyal", "lucky",
    "lunar", "magic", "mega", "mellow", "merry", "mighty", "minty", "misty", "modern", "mossy",
    "neat", "nimble", "noble", "odd", "olive", "plucky", "polar", "proud", "pure", "quick",
    "quiet", "rapid", "rare", "rosy", "royal", "ruby", "rusty", "salty", "sandy", "sharp", "shiny",
    "silent", "silver", "sleek", "sly", "smart", "snappy", "snowy", "solar", "sonic", "sparkly",
    "spicy", "steady", "stellar", "stormy", "sturdy", "sunny", "super", "sweet", "swift", "teal",
    "tidy", "tiny", "tough", "true", "turbo", "ultra", "upbeat", "vast", "velvet", "vivid", "warm",
    "wavy", "wild", "windy", "wise", "witty",
];

const ANIMALS: [&str; 128] = [
    "alpaca", "ant", "badger", "bear", "beaver", "bee", "beetle", "bison", "boar", "camel", "carp",
    "cheetah", "chipmunk", "clam", "cobra", "cod", "coyote", "crab", "crane", "cricket", "crow",
    "deer", "dingo", "dolphin", "donkey", "dove", "eagle", "eel", "elk", "emu", "falcon", "ferret",
    "finch", "firefly", "fox", "frog", "gecko", "gerbil", "gibbon", "giraffe", "goat", "gorilla",
    "gull", "hamster", "hare", "hawk", "heron", "hippo", "horse", "hyena", "ibis", "iguana",
    "jackal", "jaguar", "jay", "kiwi", "koala", "koi", "lamb", "lark", "lemur", "leopard", "lion",
    "lizard", "llama", "lobster", "lynx", "macaw", "magpie", "mantis", "marmot", "meerkat", "mink",
    "mole", "monkey", "moose", "moth", "mouse", "newt", "orca", "ostrich", "otter", "owl", "ox",
    "oyster", "panda", "parrot", "pelican", "penguin", "pigeon", "pike", "pony", "possum",
    "puffin", "puma", "python", "quail", "rabbit", "raccoon", "ram", "raven", "ray", "rhino",
    "robin", "salmon", "seal", "shark", "sheep", "shrew", "shrimp", "skunk", "sloth", "snail",
    "spider", "squid", "squirrel", "stoat", "stork", "swan", "tiger", "toad", "toucan", "trout",
    "tuna", "turtle", "viper", "walrus", "whale",
];

const VERBS: [&str; 128] = [
    "bake", "beam", "blink", "blush", "bounce", "bow", "brew", "build", "call", "carry", "carve",
    "catch", "chase", "chat", "cheer", "chew", "clap", "climb", "cook", "craft", "crawl", "dance",
    "dash", "dig", "dive", "draw", "dream", "drift", "drum", "dunk", "fetch", "fix", "flash",
    "flip", "float", "fly", "forge", "gaze", "giggle", "glide", "glow", "greet", "grin", "haul",
    "hide", "hike", "hop", "hover", "hug", "hum", "hunt", "juggle", "jump", "kick", "knit",
    "laugh", "leap", "listen", "march", "mend", "munch", "nap", "nibble", "nod", "paddle", "paint",
    "pass", "peek", "pitch", "plan", "play", "polish", "ponder", "pull", "push", "race", "read",
    "relax", "rest", "roam", "roll", "row", "run", "sail", "score", "sculpt", "seek", "sew",
    "shine", "shout", "sing", "sip", "skate", "ski", "skip", "sleep", "slide", "sneak", "sniff",
    "soar", "spark", "spin", "sprint", "stomp", "stretch", "stroll", "strum", "surf", "swim",
    "swing", "talk", "think", "throw", "tinker", "tiptoe", "toss", "trot", "twirl", "twist",
    "wander", "watch", "wave", "weave", "whistle", "wink", "wonder", "write", "yawn",
];

/// The name for `id`, from the low seven bits of its first three bytes.
pub fn username(id: &EndpointId) -> String {
    let [a, b, c, ..] = *id.as_bytes();
    let pick = |words: &[&'static str; 128], byte: u8| words[usize::from(byte & 0x7f)];
    format!(
        "{}-{}-{}",
        pick(&ADJECTIVES, a),
        pick(&ANIMALS, b),
        pick(&VERBS, c)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    #[test]
    fn names_are_stable() {
        let id = SecretKey::from_bytes(&[7; 32]).public();
        assert_eq!(username(&id), "sturdy-monkey-swim");
    }
}

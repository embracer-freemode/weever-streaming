# ID & Full ID

In Weever Streaming, Publisher/Subscriber ID can have trailing `+<RANDOM>` pattern (called it **Full ID**).
So `<ID>+<RANDOM>` is considered to be the same publisher/subscriber as `<ID>`.
In the Weever Streaming example, we use this to identify each connection from same user.
Even if a user reconnect, there will be unique **Full ID** from server perspective.

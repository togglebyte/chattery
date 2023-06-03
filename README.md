This project is a result of a viewer question from [this stream](https://www.twitch.tv/videos/1833853392?t=02h57m42s).

There are some files of relevance here:

## Server
* syncmain.rs Sync version
* main-no-username.rs  Async version of `syncmain.rs`
* main.rs Async with the addition of usernames

This is a multi room chat that was built to show case how you can do this.
There are of course many optimisations that can be done here, this was built to
demonstrate the possibilities more than anything, however I am confident that
the async version should be able to handle quite a few connection.

## Client

* client-that-works-for-now.rs (reader thread, writer thread)
* client.rs (reader writer in the same thread, sleep for 20ms)

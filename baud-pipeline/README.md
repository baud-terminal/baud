# baud-pipeline

The stages of baud between the pty and the screen, and what crosses between
them. Two stages run on two threads: the ingest thread reads and parses, the
frame thread builds and presents.

Each stage's thread context is made only on that stage's thread and never
leaves it, and a call that may block takes the frame thread's, so the ingest
thread cannot make one.

The frame handoff gives three slots, each owned by one side at a time. The
ingest thread fills a slot and publishes it, and the frame thread takes the
slot last published, each by one atomic swap; neither waits for the other.
The types say which side holds each slot: a slot reached after it was handed on, or from
two threads at once, is a compile error, not a race.

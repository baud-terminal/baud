# baud-pipeline

The frame handoff between baud's ingest thread and its frame thread: three
slots, each owned by one side at a time. The ingest thread fills a slot and
publishes it, and the frame thread takes the slot last published, each by
one atomic swap; neither waits for the other. The types say which side holds each slot:
a slot reached after it was handed on, or from two threads at once, is a
compile error, not a race.

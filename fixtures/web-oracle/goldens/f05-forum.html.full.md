
      # Thread: pipe debugging channels

      1. [op_user](/u/1) & 2h

   Has anyone wired the debugging channel to anonymous pipes
   instead of a TCP port? Looking for a containment win.
2. [reply_user](/u/2) & 1h

   Yes. Spawn the child with the pipe flags, keep FD 3 as the
   command stream and FD 4 as the event stream, and close your
   write end to shut down cleanly.

   > @op_user it also avoids port
  > collisions in containers.
3. [third_user](/u/3) & 55m

   Linking the engineering writeup from the platform team for
   anyone landing here from search.

   [notes.example.test/pipes](https://notes.example.test/pipes)

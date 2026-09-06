
    # Pipe (Unix)

      In Unix and Unix-like systems, a **pipe** is a unidirectional
 data channel between processes. Pipes were among the original
 features of Unix and remain a core composition primitive.

        ## Contents

        - 1 Creation
- 2 Anonymous pipes
- 3 Named pipes

      ## Creation

      Pipes are created with the `pipe` system call, which
 returns a pair of file descriptors. One end is for reading, the
 other for writing.

      ## Anonymous pipes

      Anonymous pipes exist only as file descriptors and cannot be
 addressed by name. They form the basis of pipelines where the
 output of one command feeds the input of the next.

      | Call | Purpose |
| --- | --- |
| pipe | create a descriptor pair |
| dup2 | duplicate onto a standard descriptor |

        [[edit](/w/index.php?title=Pipe&)]

# Decisions I must be able to defend

Fill in each answer yourself, in under five lines and without looking at the code. An answer you cannot write from memory is a gap to study, not a line to copy. Source of the questions: `NEXT_PHASES.md` section 6.

1. Why must `RequestVote` wait for the term/votedFor fsync? Describe the crash that breaks it.
   -
2. Why does a new leader need the no-op entry, and why can it only commit current-term entries by counting replicas (Figure 8)?
   -
3. Gets go through the log. How would you serve linearizable reads without it, and what breaks with leases and clock drift?
   -
4. What is the linearizability checker's complexity, why is it capped at 48 operations, and what would per-key decomposition buy?
   -
5. Seed 281: how did you know the checker was wrong and the protocol was right? Why is `Io` indeterminate but `NotLeader` definite?
   -
6. Why should anyone believe the simulator is not wrong? What does the mutant table prove, and what does it not?
   -
7. What exactly guarantees `same seed -> same digest`, and what could silently break it?
   -
8. What does ddmin guarantee (1-minimal, not minimal), and why does the minimizer need `ReplayBook`?
   -
9. Which storage faults does the WAL survive, and which can Raft not survive (fsync lies, bitrot in the durable prefix)?
   -
10. How long does the idempotency table live, and what changes when snapshots exist?
   -
11. Agent harness: why does `pass^k` here not measure model variance?
   -
12. (Optional, experimental extra.) What stops a prompt-injected tool result from becoming a privileged call, and what does the sandbox not stop?
   -

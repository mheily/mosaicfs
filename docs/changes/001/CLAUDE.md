This directory contains a scope of work related to the project. Here are the files and
the order in which they are created:


# Step 1: Initial discussion

The LLM produces these documents:

  * architecture.md - high level architectural view of the proposed changes.
  * design-notes.md - technical details pertaining to the design. This is based on architecture.md

The documents are manually committed to Git after this step.

# Step 2: Review and feedback

A new LLM session is started to review the documents produced in step 1. This produces a pair of documents:

  * review.md - the initial review of the architecture.md and review.md files, as performed by an LLM.
  * review-feedback.md - the feedback from the human reviewer about review.md
  * review-clarification.md - the response of the LLM to the comments in review-feedback.md

The documents are manually committed to Git after this step.

# Step 3: Iterate over the initial pair of documents.

A LLM edits architecture.md and design-notes.md based on the context from step 2. This is committed to Git.


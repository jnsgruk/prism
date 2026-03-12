We build a project called `contristat` (~/code/canonical/contristat) which I have been treating like a PoC.

The premise, and guiding purpose of the project is:

I'm a Senior exec at Canonical, I run 2 large Software Engineering orgs. The company has a strong sense of who is performing well, and what that should mean. I often need to go and manually collect data about how either people or teams are interacting with and performing using platforms like Github, Jira, Discourse.

I'm interested in how _teams_ are performing mostly. Think flow metrics, DORA, etc. I want to be able to compare teams and start conversations with managers that might prompt them to focus on review depth in code, or try to understand why their PRs take longer than others to merge.

I also want to be able to condense data from multiple platforms to get a broad picture of an individual's contribution in various time periods, and make comparisons with people who should be at the same level. I'm not interested really in comparing individual metrics, but looking at an aggregation of signals.

The POC has a few issues:

- Data is not persisted, so every 6 hours the wait is quite lengthy
- This is exacerbated by GitHub API rate limits
- The server, ingestion and UI are all one piece and quite tightly coupled
- The database is fairly primitive.

I'd like to scale the idea up quite dramatically. My hope is to build a platform that can track metrics across multiple teams that are active on multiple platforms, but also reason about an individual's performance.

Tracking changes to corporate structure is complicated, so I'd like to keep the ability to manage the set of users by ingesting the directory file which we've already built, and we recently used the repo automation code at ~/code/canonical/canonical-repo-automation to generate a configuration file that knows about Github teams.

I think we should move to a model whereby data is collected incrementally every few (3-6) hours and stored in a database, so that it's always ready to be queried. THis is quite a complicated and diverse dataset - and the question is when collecting information about things like pull requests, do we store more of the content in the database, or a link and a few key metrics. We'll also need to design a strategy for how to store/reason about things that are collected that might change between ingestion runs (like PRs currently open, vs closed).

The database(s?) and schema need to remain flexible. Off the top of my head I know I want to collect metrics and data from

- Multiple Discourse instances
- Github
- Launchpad (current implementation needs a lot of work - it's not collecting enough, if any, relevant data)
- Jira
- Google Drive (not yet implemented)
- Mailing Lists (ubuntu mailing lists)

But there will likely be more sources.

I'm also considering how to conduct the reasoning. We could use static metrics and data calculation like we are today, but part of me wonders whether we should instead store data, then build a more agentic system that can use an LLM to drive Python or other tools to gain insights - which should be more flexible and adapt to changing data requirements over time.

In reality, probably a mix of the two would be appropriate.

If we're going to use an agentic/AI driven approach, then we likely need to consider generating embeddings for data at the time it is ingested, and possibly enrich too. We could, for example, try to elicit a sentiment, tone or assessment of depth on a PR review from a given person - but there may be more. I suspect the level of embedding/enrichment required will vary per data source.

We're currently using a static site that uses template rendering, and I'm wondering whether that is too primtive for the kind of experience I'm trying to build.

Some things I know the architecture should do:

- Sources should be modular - it should be easy to write/onboard a new data source
- Ingestion should run seperately. I thinking about using either Restate (restate.dev) or Temporal (temporal.io) and structuring this into jobs/pipelines that can be run in isolation either manually or on schedule
- If the ingestion service misses a run, it should be able to know when it last ran and backfill data approrpriately - even if that takes a long time due to rate limit wait times
- When there are queues/processing jobs I want to be able to see data about what they're doing, how long we've waited for rate limits, what's next, etc.
- Backend/server side code should all be in Rust
- API communication should likely be with gRPC - the strong typing of messages is a nice match with Rust's type system
- We should follow a domain-driven design approach to building the system

Some open questions

- Which database(s) to use? In general I think sticking to something like PostgreSQL would be ideal, but I'm open to suggestions
- Frontend - perhaps we should move to a NextJS/React and ShadCN setup?
  - If we go this way, then we should use Typescript, Bun and a strict set of lints enforced with modern tooling like oxlint/oxfmt

I'd like something that is going to scale appropriately, but we should always favour simple and easy to understand and operate. I don't want tens of microservices just for the sake of it, but equally we should split things out where is makes sense.

Remember that at least for the foreseeable future, this will either run on my desktop machine in a VM, or on relatievly small commodity hardware at home.

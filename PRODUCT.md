# Product

## Register

product

## Users

One developer (the author) working flexible hours across several client repos, with Claude Code
and Codex agents running in parallel. They open the app at the end of a day or a week, usually
in a hurry, to turn what actually happened into hours they can log in Toggl, and now and then to
look back at what they did (journal, finds, projects).

## Product Purpose

Dev Archaeologist rebuilds a working day from evidence the user's tools already keep (agent
logs, git, calendar, foreground app, shell) and turns it into:

1. A Toggl timesheet in the user's own format (`EPIC TICKET` descriptions, quarter hours,
   days drifting around 8h to 40h a week), ready to export or send. This is the main job.
2. A reviewable day: sessions on a timeline that can be relabelled, excluded or approved.
3. A daily journal with cited finds, and a view of where time went across projects.

Success: the user gets a week's timesheet out in a couple of minutes, trusts it, and can always
see why an entry says what it says.

## Brand Personality

Quiet, exact, trustworthy. A native Mac utility, not a SaaS dashboard. Warm paper palette (the
existing one) with Toggl's clarity for time entries: lists of entries with project dots, ticket
tags, time ranges and totals.

## Anti-references

- SaaS analytics dashboards: hero metrics, card grids, gradients.
- One long settings page mixing connections, rules and preferences.
- Screens that pile every control on top of the content (the current Reports screen).
- Anything that hides why an entry got its ticket or its time.

## Design Principles

1. **The timesheet is the spine.** The path from "this week" to an exported or sent timesheet
   is short, obvious and in order; everything else supports it.
2. **Show the evidence.** Every entry, ticket and minute can be traced back to the sessions and
   events behind it, one click away, never in the way.
3. **What needs me comes first.** Items to review, fix or approve surface at the top of the
   screen they belong to, with a count in navigation.
4. **Native and familiar.** Sidebar source list, toolbar, standard controls, keyboard
   navigation. Earned familiarity over invention.
5. **Settings by purpose.** Connections, collection, work hours and timesheet rules each live
   where the user expects them, not on one scroll.

## Accessibility & Inclusion

WCAG AA contrast in both light and dark (follows the system theme). Full keyboard navigation,
visible focus, reduced motion respected. Colour never the only carrier of meaning (project dots
carry a name; states carry a label).

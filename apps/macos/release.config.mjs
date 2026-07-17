const conventionalCommits = {
  parserOpts: {
    noteKeywords: ["BREAKING CHANGE", "BREAKING CHANGES"],
  },
};

const releaseNoteSections = [
  "Highlights",
  "Bug Fixes",
  "Reliability and Maintenance",
  "Upgrade Notes",
];

const releaseNoteSectionByType = {
  feat: "Highlights",
  feature: "Highlights",
  fix: "Bug Fixes",
  upgrade: "Upgrade Notes",
  perf: "Reliability and Maintenance",
  revert: "Reliability and Maintenance",
  docs: "Reliability and Maintenance",
  style: "Reliability and Maintenance",
  chore: "Reliability and Maintenance",
  refactor: "Reliability and Maintenance",
  test: "Reliability and Maintenance",
  build: "Reliability and Maintenance",
  ci: "Reliability and Maintenance",
};

const historicalCommitFallback =
  "Historical commit message omitted; see the linked commit for details.";
const historicalNoteFallback =
  "Historical release note omitted; see the linked commit for details.";
const nonAsciiPattern = /[^\p{ASCII}]/u;

const hasNonAscii = (value) =>
  typeof value === "string" && nonAsciiPattern.test(value);

const sanitizeCommit = (commit) => ({
  ...commit,
  scope: hasNonAscii(commit.scope) ? undefined : commit.scope,
  subject: hasNonAscii(commit.subject)
    ? historicalCommitFallback
    : commit.subject,
  header: hasNonAscii(commit.header) ? historicalCommitFallback : commit.header,
});

const releaseNoteTransform = (commit) => {
  const notes = (commit.notes ?? []).map((note) => ({
    ...note,
    title: "Upgrade Notes",
    text: hasNonAscii(note.text) ? historicalNoteFallback : note.text,
  }));
  const section =
    releaseNoteSectionByType[commit.type] ??
    (notes.length > 0 ? "Upgrade Notes" : undefined);

  if (!section) return undefined;

  const sanitizedCommit = sanitizeCommit(commit);
  const shortHash = typeof commit.hash === "string"
    ? commit.hash.substring(0, 7)
    : commit.shortHash;

  return {
    notes,
    type: section,
    scope: sanitizedCommit.scope === "*" ? "" : sanitizedCommit.scope,
    shortHash,
    subject: sanitizedCommit.subject,
    header: sanitizedCommit.header,
    references: commit.references,
  };
};

const sanitizeReleaseNotesContext = (context) => ({
  ...context,
  commitGroups: context.commitGroups.map((group) => ({
    ...group,
    title: releaseNoteSections.includes(group.title)
      ? group.title
      : "Reliability and Maintenance",
    commits: group.commits.map(sanitizeCommit),
  })),
  noteGroups: context.noteGroups.map((group) => ({
    ...group,
    title: "Upgrade Notes",
    notes: group.notes.map((note) => ({
      ...note,
      text: hasNonAscii(note.text) ? historicalNoteFallback : note.text,
      commit: sanitizeCommit(note.commit),
    })),
  })),
});

const releaseNotes = {
  ...conventionalCommits,
  linkReferences: true,
  writerOpts: {
    transform: releaseNoteTransform,
    commitGroupsSort: (a, b) =>
      releaseNoteSections.indexOf(a.title) - releaseNoteSections.indexOf(b.title),
    finalizeContext: sanitizeReleaseNotesContext,
  },
};

export default {
  branches: ["main"],
  tagFormat: "app-v${version}",
  plugins: [
    [
      "@semantic-release/commit-analyzer",
      {
        ...conventionalCommits,
        releaseRules: [{ type: "revert", release: "patch" }],
      },
    ],
    ["@semantic-release/release-notes-generator", releaseNotes],
    ["@semantic-release/npm", { npmPublish: false }],
    ["@semantic-release/exec", { prepareCmd: "pnpm version:sync" }],
    [
      "@semantic-release/git",
      {
        assets: [
          "package.json",
          "src-tauri/tauri.conf.json",
          "src-tauri/Cargo.toml",
          "src-tauri/Cargo.lock",
        ],
        message:
          "chore(release): ${nextRelease.version} [skip ci]\\n\\n${nextRelease.notes}",
      },
    ],
    [
      "@semantic-release/github",
      {
        draftRelease: true,
        successComment: false,
        failComment: false,
        releasedLabels: false,
        releaseNameTemplate: "Sortlytic v<%= nextRelease.version %>",
      },
    ],
  ],
};

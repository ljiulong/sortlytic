const conventionalCommits = {
  parserOpts: {
    noteKeywords: ["BREAKING CHANGE", "BREAKING CHANGES"],
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
    ["@semantic-release/release-notes-generator", conventionalCommits],
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
        successComment: false,
        failComment: false,
        releasedLabels: false,
        releaseNameTemplate: "Sortlytic v<%= nextRelease.version %>",
      },
    ],
  ],
};

import { createRequire } from 'node:module'
import { readFile } from 'node:fs/promises'
import { dirname } from 'node:path'
import { pathToFileURL } from 'node:url'

import { describe, expect, it } from 'vitest'
import {
  selectPushRange,
  validateReleaseTitles,
} from '../../scripts/validate-english-release.mjs'

import releaseConfig from '../release.config.mjs'

const require = createRequire(import.meta.url)

describe('semantic-release configuration', () => {
  it('validates every main push before semantic-release decides whether to publish', async () => {
    const workflow = await readFile(
      new URL('../../../.github/workflows/release-macos.yml', import.meta.url),
      'utf8',
    )

    expect(workflow).not.toContain('github.event.head_commit.message')
    expect(workflow).toContain('uses: ./.github/workflows/ci.yml')
  })

  it('publishes the stable release only after both architecture assets are uploaded', async () => {
    const githubPlugin = releaseConfig.plugins.find(
      (plugin) => Array.isArray(plugin) && plugin[0] === '@semantic-release/github',
    )
    const workflow = await readFile(
      new URL('../../../.github/workflows/release-macos.yml', import.meta.url),
      'utf8',
    )

    expect(githubPlugin?.[1]).toMatchObject({ draftRelease: true })
    expect(workflow).toContain("releaseDraft: ${{ inputs.rebuild_tag == '' }}")
    expect(workflow).toContain('finalize-release:')
    expect(workflow).toContain('needs: [release, build-and-release]')
    expect(workflow).toContain('gh release edit "$TAG" --repo "$GITHUB_REPOSITORY" --draft=false --latest')
  })

  it('normalizes updater URLs before publishing the stable release', async () => {
    const workflow = await readFile(
      new URL('../../../.github/workflows/release-macos.yml', import.meta.url),
      'utf8',
    )

    expect(workflow).toContain('.apiUrl')
    expect(workflow).toContain(
      'gh release view "$TAG" --repo "$GITHUB_REPOSITORY" --json assets,body',
    )
    expect(workflow).not.toContain('gh api "repos/$GITHUB_REPOSITORY/releases/tags/$TAG"')
    expect(workflow).toContain('Normalize updater manifest')
    expect(workflow).toContain(
      'gh release upload "$TAG" "$MANIFEST" --repo "$GITHUB_REPOSITORY" --clobber',
    )
    expect(workflow).toContain('startswith("https://github.com/")')
  })

  it('repairs the updater manifest without republishing a recovery tag', async () => {
    const workflow = await readFile(
      new URL('../../../.github/workflows/release-macos.yml', import.meta.url),
      'utf8',
    )
    const finalizeJob = workflow.slice(workflow.indexOf('  finalize-release:'))

    expect(finalizeJob).toContain("if: needs.release.outputs.published == 'true'")
    expect(finalizeJob).toMatch(
      /- name: Publish verified draft release\n\s+if: >-\n\s+github\.event_name != 'workflow_dispatch' \|\| inputs\.rebuild_tag == ''/,
    )
  })

  it('pins every third-party Action to a full commit SHA', async () => {
    const workflow = await readFile(
      new URL('../../../.github/workflows/release-macos.yml', import.meta.url),
      'utf8',
    )
    const actionRefs = [...workflow.matchAll(/^\s*uses:\s+([^@\s]+)@([^\s#]+)/gm)]

    expect(actionRefs.length).toBeGreaterThan(0)
    for (const [, action, ref] of actionRefs) {
      if (!action.startsWith('./')) expect(ref, action).toMatch(/^[a-f0-9]{40}$/)
    }
  })

  it('runs the container-based cargo-deny Action on Linux only', async () => {
    const workflow = await readFile(
      new URL('../../../.github/workflows/ci.yml', import.meta.url),
      'utf8',
    )

    expect(workflow).toMatch(
      /audit-rust-dependencies:\n[\s\S]*?runs-on: ubuntu-latest[\s\S]*?uses: EmbarkStudios\/cargo-deny-action@/,
    )
  })

  it('generates English release notes for feature and fix commits', async () => {
    const semanticReleaseEntry = require.resolve('semantic-release')
    const generatorEntry = require.resolve('@semantic-release/release-notes-generator', {
      paths: [dirname(semanticReleaseEntry)],
    })
    const { generateNotes } = await import(pathToFileURL(generatorEntry).href)
    const generatorPlugin = releaseConfig.plugins.find(
      (plugin) => Array.isArray(plugin) && plugin[0] === '@semantic-release/release-notes-generator',
    )

    expect(generatorPlugin).toBeDefined()
    const notes = await generateNotes(generatorPlugin?.[1] ?? {}, {
      cwd: process.cwd(),
      commits: [
        { hash: '1111111111111111', message: 'feat: add collection target' },
        { hash: '2222222222222222', message: 'fix: preserve request limit' },
      ],
      lastRelease: { gitTag: 'app-v0.1.5' },
      nextRelease: { gitTag: 'app-v0.2.0', version: '0.2.0' },
      options: { repositoryUrl: 'https://github.com/ljiulong/sortlytic.git' },
    })

    expect(notes).toContain('Highlights')
    expect(notes).toContain('Bug Fixes')
    expect(notes).toContain('add collection target')
    expect(notes).toContain('preserve request limit')
  })

  it('uses an English fallback for historical non-ASCII commit subjects', async () => {
    const semanticReleaseEntry = require.resolve('semantic-release')
    const generatorEntry = require.resolve('@semantic-release/release-notes-generator', {
      paths: [dirname(semanticReleaseEntry)],
    })
    const { generateNotes } = await import(pathToFileURL(generatorEntry).href)
    const generatorPlugin = releaseConfig.plugins.find(
      (plugin) => Array.isArray(plugin) && plugin[0] === '@semantic-release/release-notes-generator',
    )

    const notes = await generateNotes(generatorPlugin?.[1] ?? {}, {
      cwd: process.cwd(),
      commits: [
        { hash: '3333333333333333', message: 'feat: 修复中文历史标题' },
        { hash: '4444444444444444', message: 'fix: preserve English title' },
      ],
      lastRelease: { gitTag: 'app-v0.2.0' },
      nextRelease: { gitTag: 'app-v0.2.1', version: '0.2.1' },
      options: { repositoryUrl: 'https://github.com/ljiulong/sortlytic.git' },
    })

    expect(notes).toContain('Highlights')
    expect(notes).toContain('Historical commit message omitted')
    expect(notes).toContain('preserve English title')
    expect(notes).not.toContain('修复中文历史标题')
  })

  it('blocks non-ASCII release metadata and accepts English titles', () => {
    expect(() => validateReleaseTitles(['feat: add an English feature'])).not.toThrow()
    expect(() => validateReleaseTitles(['feat: 新功能'])).toThrow('Release blocked')
  })

  it('excludes commits that predate the English metadata policy from the first validated push', () => {
    const ancestors = new Set(['cutover:after'])
    const isAncestor = (ancestor: string, descendant: string) => (
      ancestors.has(`${ancestor}:${descendant}`)
    )

    expect(selectPushRange({
      after: 'after',
      before: 'legacy-remote',
      cutover: 'cutover',
      isAncestor,
    })).toBe('cutover^..after')

    ancestors.add('cutover:current-remote')
    expect(selectPushRange({
      after: 'next',
      before: 'current-remote',
      cutover: 'cutover',
      isAncestor,
    })).toBe('current-remote..next')
  })
})

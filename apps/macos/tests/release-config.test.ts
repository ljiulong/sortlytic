import { createRequire } from 'node:module'
import { readFile } from 'node:fs/promises'
import { dirname } from 'node:path'
import { pathToFileURL } from 'node:url'

import { describe, expect, it } from 'vitest'

import releaseConfig from '../release.config.mjs'

const require = createRequire(import.meta.url)

describe('semantic-release 配置', () => {
  it('每次 main 推送都先验证并交给 semantic-release 判断是否发版', async () => {
    const workflow = await readFile(
      new URL('../../../.github/workflows/release-macos.yml', import.meta.url),
      'utf8',
    )

    expect(workflow).not.toContain('github.event.head_commit.message')
    expect(workflow).toContain('uses: ./.github/workflows/ci.yml')
  })

  it('双架构产物全部上传后才公开稳定版', async () => {
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

  it('公开稳定版前把更新清单改为直接下载地址', async () => {
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
    expect(workflow).toContain('gh release upload "$TAG" "$MANIFEST" --clobber')
    expect(workflow).toContain('startswith("https://github.com/")')
  })

  it('重打包已有标签时仍修复更新清单但不重复发布', async () => {
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

  it('发版工作流把所有第三方 Action 固定到完整提交 SHA', async () => {
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

  it('容器型 cargo-deny Action 只在 Linux runner 上运行', async () => {
    const workflow = await readFile(
      new URL('../../../.github/workflows/ci.yml', import.meta.url),
      'utf8',
    )

    expect(workflow).toMatch(
      /audit-rust-dependencies:\n[\s\S]*?runs-on: ubuntu-latest[\s\S]*?uses: EmbarkStudios\/cargo-deny-action@/,
    )
  })

  it('为功能与修复提交生成非空发版说明', async () => {
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

    expect(notes).toContain('Features')
    expect(notes).toContain('Bug Fixes')
    expect(notes).toContain('add collection target')
    expect(notes).toContain('preserve request limit')
  })
})

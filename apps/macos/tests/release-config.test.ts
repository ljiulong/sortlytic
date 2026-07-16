import { createRequire } from 'node:module'
import { dirname } from 'node:path'
import { pathToFileURL } from 'node:url'

import { describe, expect, it } from 'vitest'

import releaseConfig from '../release.config.mjs'

const require = createRequire(import.meta.url)

describe('semantic-release 配置', () => {
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

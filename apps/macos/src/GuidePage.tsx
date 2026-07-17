import {
  BadgeCheck,
  CheckCircle2,
  ExternalLink,
  KeyRound,
  MonitorCheck,
  ShieldCheck,
} from 'lucide-react'
import './GuidePage.css'

type GuidePageProps = {
  onOpenSettings: () => void
}

const setupSteps = [
  {
    title: '注册并验证账号',
    detail: '打开 TikHub 注册页，完成邮箱验证。新账号通常会获得少量免费额度，适合先做连通性测试。',
  },
  {
    title: '创建 API Token',
    detail: '登录用户中心后进入 API Token 菜单，新建 Token 并立即复制。Token 只展示给你本人，不要放进文档或聊天记录。',
  },
  {
    title: '添加到本地应用',
    detail: '在 Sortlytic 设置中选择适合当前网络的 API 域名，粘贴 Token，然后保存并测试真实账号与额度。',
  },
  {
    title: '先小样本验证',
    detail: '首次使用 10 到 50 条记录，确认平台、国家地区、筛选条件和成本上限正确后再扩大任务。',
  },
]

const safetyChecks = [
  'Token 只保存在本机系统安全存储中，界面不明文回显。',
  '中国大陆网络优先尝试 api.tikhub.dev，国际网络优先尝试 api.tikhub.io。',
  '不同端点价格不同，正式采集前先确认 API Marketplace 与实时计价。',
  '首次任务使用小样本，并在任务页确认运行后再产生正式采集费用。',
]

const officialResources = [
  { href: 'https://user.tikhub.io/register', label: '创建 TikHub 账号' },
  { href: 'https://user.tikhub.io/login', label: '登录用户中心' },
  { href: 'https://docs.tikhub.io/', label: '查看 API 文档' },
  { href: 'https://tikhub.io/getting-started', label: '首次调用指南' },
  { href: 'https://tikhub.io/pricing', label: '价格与免费额度' },
]

function GuidePage({ onOpenSettings }: GuidePageProps) {
  return (
    <section className="guide-page" aria-label="使用指南">
      <main className="guide-page__main">
        <header className="guide-intro">
          <div>
            <p className="eyebrow">开始使用 TikHub</p>
            <h2>从账号注册到运行第一条任务</h2>
            <p>按顺序完成四步，所有密钥、任务和结果都保留在当前 macOS 工作区。</p>
          </div>
          <span className="status-pill" data-tone="info">
            <MonitorCheck size={13} aria-hidden="true" />
            适用于本地工作区
          </span>
        </header>

        <ol className="guide-flow">
          {setupSteps.map((step, index) => (
            <li className="guide-step" key={step.title}>
              <div className="guide-step__rail" aria-hidden="true">
                <span>{String(index + 1).padStart(2, '0')}</span>
              </div>
              <div className="guide-step__content">
                <h3>{step.title}</h3>
                <p>{step.detail}</p>
                {index === 2 ? (
                  <>
                    <dl className="guide-step__facts">
                      <div>
                        <dt>API 域名</dt>
                        <dd>国际网络使用 api.tikhub.io，中国大陆网络使用 api.tikhub.dev。</dd>
                      </div>
                      <div>
                        <dt>API Token</dt>
                        <dd>保存后输入框会清空，数据库只记录系统安全存储引用。</dd>
                      </div>
                      <div>
                        <dt>连通测试</dt>
                        <dd>成功后显示真实账号、充值余额、免费额度和今日用量。</dd>
                      </div>
                    </dl>
                    <button className="ghost-button" type="button" onClick={onOpenSettings}>
                      <KeyRound size={16} aria-hidden="true" />
                      打开设置
                    </button>
                  </>
                ) : null}
              </div>
            </li>
          ))}
        </ol>

        <section className="guide-checklist" aria-labelledby="guide-checklist-heading">
          <header>
            <div>
              <p className="eyebrow">运行前确认</p>
              <h2 id="guide-checklist-heading">安全与成本边界</h2>
            </div>
            <ShieldCheck size={19} aria-hidden="true" />
          </header>
          <ul>
            {safetyChecks.map((item) => (
              <li key={item}>
                <CheckCircle2 size={16} aria-hidden="true" />
                <span>{item}</span>
              </li>
            ))}
          </ul>
          <p className="guide-checklist__note">
            测试失败时，依次检查 Token 完整性、邮箱验证、当前网络对应域名和可用额度。
          </p>
        </section>
      </main>

      <aside className="guide-page__sidebar" aria-label="官方资源与请求格式">
        <section className="guide-resources" aria-labelledby="guide-resources-heading">
          <header>
            <div>
              <p className="eyebrow">官方资源</p>
              <h2 id="guide-resources-heading">注册与文档</h2>
            </div>
            <BadgeCheck size={18} aria-hidden="true" />
          </header>
          <nav aria-label="TikHub 官方资源">
            {officialResources.map((resource) => (
              <GuideLink {...resource} key={resource.href} />
            ))}
          </nav>
        </section>

        <section className="guide-token-block" aria-labelledby="guide-token-heading">
          <header>
            <div>
              <p className="eyebrow">请求格式</p>
              <h2 id="guide-token-heading">Token 用法</h2>
            </div>
            <KeyRound size={18} aria-hidden="true" />
          </header>
          <dl>
            <div>
              <dt>请求头</dt>
              <dd>Authorization</dd>
            </div>
            <div>
              <dt>格式</dt>
              <dd>Bearer YOUR_API_KEY</dd>
            </div>
            <div>
              <dt>本应用保存方式</dt>
              <dd>只保存系统安全存储引用，不把明文 Token 写入任务或导出文件。</dd>
            </div>
          </dl>
        </section>
      </aside>
    </section>
  )
}

function GuideLink({ href, label }: { href: string; label: string }) {
  return (
    <a className="guide-resource-link" href={href} rel="noreferrer" target="_blank">
      <span>{label}</span>
      <ExternalLink size={15} aria-hidden="true" />
    </a>
  )
}

export default GuidePage

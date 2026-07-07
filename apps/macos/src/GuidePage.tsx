import {
  BadgeCheck,
  CheckCircle2,
  ExternalLink,
  KeyRound,
  MonitorCheck,
  ShieldCheck,
} from 'lucide-react'

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
    detail: '回到本应用的 TikHub 设置，选择 API 域名，粘贴 Token，然后点击保存并测试。',
  },
  {
    title: '先小样本验证',
    detail: '用 10 到 50 条记录做首次采集，确认平台、关键词、国家地区和成本上限都正确后再扩大任务。',
  },
]

const safetyChecks = [
  'Token 只保存在本机系统安全存储中，界面不明文回显。',
  '请求头格式为 Authorization: Bearer YOUR_API_KEY。',
  '中国大陆网络优先尝试 api.tikhub.dev，国际网络优先尝试 api.tikhub.io。',
  '不同端点价格不同，正式采集前先看 API Marketplace 和价格说明。',
]

function GuidePage({ onOpenSettings }: GuidePageProps) {
  return (
    <section className="main-grid" aria-label="使用指南">
      <div className="main-column">
        <section className="glass-panel">
          <div className="section-heading">
            <div>
              <p className="eyebrow">使用指南</p>
              <h2>从 API 注册到本地添加</h2>
            </div>
            <span className="status-pill" data-tone="info">
              <MonitorCheck size={13} aria-hidden="true" />
              适用于 macOS 本地工作区
            </span>
          </div>
          <div className="connection-grid">
            {setupSteps.map((step, index) => (
              <article className="connection-card" key={step.title}>
                <div className="connection-icon" data-tone="info">
                  <span className="mono">{index + 1}</span>
                </div>
                <div>
                  <p className="connection-name">{step.title}</p>
                  <p className="muted-text">{step.detail}</p>
                </div>
              </article>
            ))}
          </div>
        </section>

        <section className="glass-panel">
          <div className="section-heading">
            <div>
              <p className="eyebrow">添加流程</p>
              <h2>在应用内保存并测试 Token</h2>
            </div>
            <button className="ghost-button" type="button" onClick={onOpenSettings}>
              <KeyRound size={16} aria-hidden="true" />
              <span>打开设置</span>
            </button>
          </div>
          <div className="plan-grid">
            <InfoBlock label="1. API 域名" value="国际网络选 api.tikhub.io，中国大陆网络选 api.tikhub.dev。" />
            <InfoBlock label="2. API Token" value="粘贴从用户中心复制的 Token，保存后输入框会清空。" />
            <InfoBlock label="3. 连通测试" value="测试成功后会显示账号、免费额度和邮箱验证状态。" />
          </div>
          <div className="plan-preview">
            <p className="muted-text">
              如果测试失败，优先检查 Token 是否完整、邮箱是否完成验证、域名是否适合当前网络，再确认账户余额是否足够覆盖目标端点。
            </p>
          </div>
        </section>

        <section className="glass-panel">
          <div className="section-heading">
            <div>
              <p className="eyebrow">安全与成本</p>
              <h2>上线前的最低检查清单</h2>
            </div>
            <span className="status-pill" data-tone="success">
              <ShieldCheck size={13} aria-hidden="true" />
              建议逐项确认
            </span>
          </div>
          <div className="task-list">
            {safetyChecks.map((item) => (
              <article className="task-row" key={item}>
                <div>
                  <h3>{item}</h3>
                  <p>配置、测试和采集都在本地工作区完成。</p>
                </div>
                <span className="status-pill" data-tone="success">
                  <CheckCircle2 size={13} aria-hidden="true" />
                  已纳入
                </span>
              </article>
            ))}
          </div>
        </section>
      </div>

      <aside className="inspector" aria-label="外部文档与排错">
        <section className="glass-panel compact-panel">
          <div className="section-heading">
            <div>
              <p className="eyebrow">官方入口</p>
              <h2>注册与文档</h2>
            </div>
            <BadgeCheck size={18} aria-hidden="true" />
          </div>
          <div className="export-grid">
            <GuideLink href="https://user.tikhub.io/register" label="创建 TikHub 账号" />
            <GuideLink href="https://user.tikhub.io/login" label="登录用户中心" />
            <GuideLink href="https://docs.tikhub.io/" label="查看 API 文档" />
            <GuideLink href="https://tikhub.io/getting-started" label="首次调用指南" />
            <GuideLink href="https://tikhub.io/pricing" label="价格与免费额度" />
          </div>
        </section>

        <section className="glass-panel compact-panel">
          <div className="section-heading">
            <div>
              <p className="eyebrow">请求格式</p>
              <h2>Token 用法</h2>
            </div>
            <KeyRound size={18} aria-hidden="true" />
          </div>
          <div className="evidence-body">
            <dl>
              <div>
                <dt>请求头</dt>
                <dd className="mono">Authorization</dd>
              </div>
              <div>
                <dt>格式</dt>
                <dd className="mono">Bearer YOUR_API_KEY</dd>
              </div>
              <div>
                <dt>本应用保存方式</dt>
                <dd>只保存安全存储引用，不把明文 Token 写入导出报告。</dd>
              </div>
            </dl>
          </div>
        </section>
      </aside>
    </section>
  )
}

function InfoBlock({ label, value }: { label: string; value: string }) {
  return (
    <div className="info-line">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  )
}

function GuideLink({ href, label }: { href: string; label: string }) {
  return (
    <a className="export-item" href={href} rel="noreferrer" target="_blank">
      <div className="connection-icon" data-tone="info">
        <ExternalLink size={15} aria-hidden="true" />
      </div>
      <div>
        <span>外部链接</span>
        <strong>{label}</strong>
      </div>
    </a>
  )
}

export default GuidePage

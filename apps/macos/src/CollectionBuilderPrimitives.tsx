import {
  Activity,
  AlertTriangle,
  BadgeCheck,
  CheckCircle2,
} from 'lucide-react'

export function PlanFact({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  )
}

export function StatusPill({ tone, label }: { tone: string; label: string }) {
  return (
    <span className="status-pill" data-tone={tone}>
      {iconForTone(tone)}
      {label}
    </span>
  )
}

function iconForTone(tone: string) {
  if (tone === 'success') return <CheckCircle2 size={13} aria-hidden="true" />
  if (tone === 'danger') return <AlertTriangle size={13} aria-hidden="true" />
  if (tone === 'warning') return <Activity size={13} aria-hidden="true" />
  return <BadgeCheck size={13} aria-hidden="true" />
}

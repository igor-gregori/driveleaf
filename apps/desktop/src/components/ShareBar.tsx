/** A small bar showing an item's share of the space around it. */
export function ShareBar({ share }: { share: number }) {
  return (
    <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
      <div
        className="h-full rounded-full bg-primary"
        style={{ width: `${Math.min(100, share * 100)}%` }}
      />
    </div>
  );
}

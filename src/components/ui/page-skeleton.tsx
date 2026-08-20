export function PageSkeleton() {
  return (
    <div className="space-y-4 animate-pulse">
      <div className="h-8 w-48 bg-muted rounded" />
      <div className="h-4 w-96 bg-muted rounded" />
      <div className="h-64 bg-muted rounded" />
    </div>
  );
}

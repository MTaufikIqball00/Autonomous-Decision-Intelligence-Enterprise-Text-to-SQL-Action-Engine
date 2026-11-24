declare module "lucide-react" {
  import type { ForwardRefExoticComponent, RefAttributes, SVGProps } from "react";

  export interface LucideProps extends SVGProps<SVGSVGElement> {
    size?: string | number;
    absoluteStrokeWidth?: boolean;
  }

  export type LucideIcon = ForwardRefExoticComponent<
    Omit<LucideProps, "ref"> & RefAttributes<SVGSVGElement>
  >;

  export const Activity: LucideIcon;
  export const AlertTriangle: LucideIcon;
  export const ArrowRight: LucideIcon;
  export const BarChart3: LucideIcon;
  export const BookOpen: LucideIcon;
  export const Bookmark: LucideIcon;
  export const BookmarkPlus: LucideIcon;
  export const CheckCircle2: LucideIcon;
  export const Clock: LucideIcon;
  export const Cpu: LucideIcon;
  export const Database: LucideIcon;
  export const Download: LucideIcon;
  export const FileText: LucideIcon;
  export const GitMerge: LucideIcon;
  export const HelpCircle: LucideIcon;
  export const History: LucideIcon;
  export const Info: LucideIcon;
  export const Key: LucideIcon;
  export const LayoutDashboard: LucideIcon;
  export const LayoutGrid: LucideIcon;
  export const Lightbulb: LucideIcon;
  export const Loader2: LucideIcon;
  export const Lock: LucideIcon;
  export const LogOut: LucideIcon;
  export const Menu: LucideIcon;
  export const MessageSquarePlus: LucideIcon;
  export const MessageSquareText: LucideIcon;
  export const Play: LucideIcon;
  export const Plus: LucideIcon;
  export const RefreshCw: LucideIcon;
  export const Search: LucideIcon;
  export const Send: LucideIcon;
  export const ShieldAlert: LucideIcon;
  export const ShieldCheck: LucideIcon;
  export const Sparkles: LucideIcon;
  export const Table2: LucideIcon;
  export const Trash2: LucideIcon;
  export const Wrench: LucideIcon;
  export const X: LucideIcon;
  export const XCircle: LucideIcon;
}

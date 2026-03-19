import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  GitPullRequest,
  MessageSquare,
  MessagesSquare,
  TicketCheck,
  Users,
  UsersRound,
} from "lucide-react";
import { Link } from "react-router-dom";

import type { OrgDeliverySummary } from "@ps/api/gen/prism/v1/insights_pb";

interface CardDef {
  key: string;
  label: string;
  icon: typeof GitPullRequest;
  getValue: (d: OrgDeliverySummary) => number;
  show: (d: OrgDeliverySummary) => boolean;
  link?: string;
}

const cards: CardDef[] = [
  {
    key: "prs",
    label: "PRs Merged",
    icon: GitPullRequest,
    getValue: (d: OrgDeliverySummary): number => d.totalPrsMerged,
    show: (): boolean => true,
  },
  {
    key: "reviews",
    label: "Reviews Given",
    icon: MessageSquare,
    getValue: (d: OrgDeliverySummary): number => d.totalReviews,
    show: (): boolean => true,
  },
  {
    key: "jira",
    label: "Jira Closed",
    icon: TicketCheck,
    getValue: (d: OrgDeliverySummary): number => d.totalJiraClosed,
    show: (d: OrgDeliverySummary): boolean => d.totalJiraClosed > 0,
  },
  {
    key: "topics",
    label: "Discourse Topics",
    icon: MessagesSquare,
    getValue: (d: OrgDeliverySummary): number => d.totalDiscourseTopics,
    show: (d: OrgDeliverySummary): boolean =>
      d.totalDiscourseTopics > 0 || d.totalDiscoursePosts > 0,
  },
  {
    key: "contributors",
    label: "Active Contributors",
    icon: Users,
    getValue: (d: OrgDeliverySummary): number => d.activeContributors,
    show: (): boolean => true,
  },
  {
    key: "teams",
    label: "Active Teams",
    icon: UsersRound,
    getValue: (d: OrgDeliverySummary): number => d.activeTeams,
    show: (): boolean => true,
    link: "/teams",
  },
];

export const DeliverySummaryCards = ({
  delivery,
}: {
  delivery: OrgDeliverySummary;
}): React.ReactElement => {
  const visibleCards = cards.filter((c) => c.show(delivery));

  return (
    <div className="grid grid-cols-2 gap-4 lg:grid-cols-3 xl:grid-cols-6">
      {visibleCards.map((c) => {
        const value = c.getValue(delivery);
        const card = (
          <Card className={c.link ? "transition-colors hover:border-foreground/20" : undefined}>
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
              <CardTitle className="text-sm font-medium text-muted-foreground">{c.label}</CardTitle>
              <c.icon className="size-4 text-muted-foreground" />
            </CardHeader>
            <CardContent>
              <span className="text-2xl font-semibold tabular-nums">{value.toLocaleString()}</span>
            </CardContent>
          </Card>
        );

        if (c.link) {
          return (
            <Link key={c.key} to={c.link} className="contents">
              {card}
            </Link>
          );
        }
        return <div key={c.key}>{card}</div>;
      })}
    </div>
  );
};

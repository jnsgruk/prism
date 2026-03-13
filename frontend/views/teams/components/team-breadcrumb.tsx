import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from "@/components/ui/breadcrumb";
import { Fragment } from "react";

import type { Team } from "@ps/api/gen/prism/v1/org_pb";

import { getAncestors } from "@/views/teams/hooks/use-teams";

export const TeamBreadcrumb = ({
  roots,
  selectedTeamId,
  onSelect,
}: {
  roots: Team[];
  selectedTeamId: string;
  onSelect: (teamId: string) => void;
}): React.ReactElement | null => {
  const ancestors = getAncestors(roots, selectedTeamId);
  if (ancestors.length === 0) return null;

  return (
    <Breadcrumb>
      <BreadcrumbList>
        {ancestors.map((team, i) => {
          const isLast = i === ancestors.length - 1;
          return (
            <Fragment key={team.id}>
              {i > 0 && <BreadcrumbSeparator />}
              <BreadcrumbItem>
                {isLast ? (
                  <BreadcrumbPage>{team.name}</BreadcrumbPage>
                ) : (
                  <BreadcrumbLink
                    render={<button type="button" />}
                    onClick={() => onSelect(team.id)}
                  >
                    {team.name}
                  </BreadcrumbLink>
                )}
              </BreadcrumbItem>
            </Fragment>
          );
        })}
      </BreadcrumbList>
    </Breadcrumb>
  );
};

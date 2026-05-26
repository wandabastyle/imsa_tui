import { useEffect, useState } from 'react';

interface TableDimensions {
  rowHeight: number;
  viewportHeight: number;
}

export const DEFAULT_ROW_HEIGHT = 24;
export const DEFAULT_VIEWPORT_HEIGHT = 0;

export const useTableDimensions = (
  scrollContainerRef: React.RefObject<HTMLDivElement | null>,
): TableDimensions | null => {
  const [dimensions, setDimensions] = useState<TableDimensions | null>(null);

  useEffect(() => {
    const container = scrollContainerRef.current;
    const viewportHeight = container?.clientHeight ?? DEFAULT_VIEWPORT_HEIGHT;

    const resizeObserver = new ResizeObserver((): void => {
      if (container === null) {
        return;
      }

      setDimensions((prev) => {
        if (prev === null) {
          return prev;
        }

        return { rowHeight: prev.rowHeight, viewportHeight: container.clientHeight };
      });
    });

    if (container !== null) {
      resizeObserver.observe(container);
    }

    const measureContainer = document.createElement('div');
    measureContainer.style.position = 'absolute';
    measureContainer.style.top = '-9999px';
    measureContainer.style.left = '-9999px';
    measureContainer.style.visibility = 'hidden';
    measureContainer.style.whiteSpace = 'nowrap';

    const measureTable = document.createElement('table');
    const measureTbody = document.createElement('tbody');
    const hiddenMeasureRow = document.createElement('tr');
    const measureCell = document.createElement('td');
    measureCell.textContent = 'M';
    measureCell.style.padding = '0';
    measureCell.style.border = 'none';
    measureCell.style.margin = '0';
    hiddenMeasureRow.append(measureCell);
    measureTbody.append(hiddenMeasureRow);
    measureTable.append(measureTbody);
    measureContainer.append(measureTable);
    document.body.append(measureContainer);

    const rowHeight = hiddenMeasureRow.offsetHeight || DEFAULT_ROW_HEIGHT;

    measureContainer.remove();

    setDimensions({
      rowHeight,
      viewportHeight,
    });

    return (): void => {
      resizeObserver.disconnect();
      const parent = hiddenMeasureRow.parentElement;
      if (parent !== null) {
        parent.remove();
      }
    };
  }, [scrollContainerRef]);

  return dimensions;
};

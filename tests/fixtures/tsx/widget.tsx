import React from 'react';
import { multiply } from './sample.js';

export interface WidgetProps {
  count: number;
}

export class WidgetContainer extends React.Component<WidgetProps> {
  calculateCount(multiplier: number): number {
    return multiply(this.props.count, multiplier);
  }

  render() {
    const val = this.calculateCount(2);
    return <div className="widget">{val}</div>;
  }
}

export const WidgetView: React.FC<WidgetProps> = (props) => {
  return <WidgetContainer count={props.count} />;
};

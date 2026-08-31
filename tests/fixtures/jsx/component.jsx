import React from 'react';
import { computeTotal } from './sample.js';

export class ButtonComponent extends React.Component {
  render() {
    computeTotal(1, 2);
    return <button onClick={() => computeTotal(3, 4)}>Click</button>;
  }
}

export function Header(props) {
  return (
    <div className="header">
      <h1>Title</h1>
      <ButtonComponent />
    </div>
  );
}

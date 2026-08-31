import { createRoot } from 'react-dom/client';
import { ReactFlow, ReactFlowProvider, Background } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import './index.css';
import FlowNodeComponent from './components/flows/canvas/FlowNodeComponent';
import { StepNumberProvider } from './components/flows/canvas/stepNumbers';

const mk = (id: string, kind: string, name: string, y: number, outs = ['main'], ins = ['main']) => ({
  id, type: 'flowNode', position: { x: 120, y },
  data: { kind, name, config: {}, inputPorts: ins, outputPorts: outs },
});

const nodes = [
  mk('t', 'trigger', 'Every morning at 9am', 0, ['main'], []),
  mk('a', 'agent', 'Summarise unread inbox', 200),
  mk('c', 'condition', 'Anything urgent?', 400, ['true', 'false']),
  mk('s', 'tool_call', 'Send to #standup', 620),
];
const edges = [
  { id: 'e1', source: 't', target: 'a', sourceHandle: 'main', targetHandle: 'main' },
  { id: 'e2', source: 'a', target: 'c', sourceHandle: 'main', targetHandle: 'main' },
  { id: 'e3', source: 'c', target: 's', sourceHandle: 'true', targetHandle: 'main' },
];

createRoot(document.getElementById('root')!).render(
  <div className="h-screen w-screen">
    <ReactFlowProvider>
      <StepNumberProvider nodes={nodes as never} edges={edges as never}>
        <ReactFlow
          nodes={nodes as never}
          edges={edges as never}
          nodeTypes={{ flowNode: FlowNodeComponent } as never}
          fitView
          fitViewOptions={{ padding: 0.15 }}>
          <Background />
        </ReactFlow>
      </StepNumberProvider>
    </ReactFlowProvider>
  </div>
);

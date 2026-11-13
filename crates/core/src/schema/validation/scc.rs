//! Compute strongly connected components of a directed graph iteratively.

struct Frame {
    node: usize,
    next_edge: usize,
}

pub(super) fn strongly_connected_components(graph: &[Vec<usize>]) -> Vec<usize> {
    let finish_order = finish_order(graph);
    let reverse = reverse_graph(graph);
    let mut components = vec![usize::MAX; graph.len()];
    let mut component = 0;
    for &start in finish_order.iter().rev() {
        if components[start] != usize::MAX {
            continue;
        }
        components[start] = component;
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            for &target in &reverse[node] {
                if components[target] == usize::MAX {
                    components[target] = component;
                    stack.push(target);
                }
            }
        }
        component += 1;
    }
    components
}

fn finish_order(graph: &[Vec<usize>]) -> Vec<usize> {
    let mut visited = vec![false; graph.len()];
    let mut order = Vec::with_capacity(graph.len());
    for start in 0..graph.len() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut stack = vec![Frame {
            node: start,
            next_edge: 0,
        }];
        while let Some(frame) = stack.last_mut() {
            if frame.next_edge == graph[frame.node].len() {
                order.push(frame.node);
                stack.pop();
                continue;
            }
            let target = graph[frame.node][frame.next_edge];
            frame.next_edge += 1;
            if !visited[target] {
                visited[target] = true;
                stack.push(Frame {
                    node: target,
                    next_edge: 0,
                });
            }
        }
    }
    order
}

fn reverse_graph(graph: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut reverse = vec![Vec::new(); graph.len()];
    for (source, edges) in graph.iter().enumerate() {
        for &target in edges {
            reverse[target].push(source);
        }
    }
    reverse
}

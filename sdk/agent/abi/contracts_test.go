// INPUT: testing
// OUTPUT: Task transition contract regressions
// POS: [L2.3 agent contracts] Agent task semantics stay above the kernel and reject terminal-state resurrection.
package abi

import "testing"

func TestTaskTransitionsAreExplicit(t *testing.T) {
	task := Task{ID: "task", Status: TaskPending}
	if err := task.Transition(TaskSucceeded); err == nil {
		t.Fatal("pending task skipped execution")
	}
	if err := task.Transition(TaskRunning); err != nil {
		t.Fatal(err)
	}
	if err := task.Transition(TaskSucceeded); err != nil {
		t.Fatal(err)
	}
	if err := task.Transition(TaskRunning); err == nil {
		t.Fatal("terminal task resurrected")
	}
}

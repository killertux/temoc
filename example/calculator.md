### Calculator example

This is the example of an acceptance test for a calculator.

So, in this calculator we can sum values like this:

[//]: # (import SlimServer.Fixtures)

[//]: # (decisionTable Calculator)

| a  | b   | sum? |
|----|-----|------|
| 1  | 2   | 3    |
| 2  | 2   | 4    |
| 10 | 5   | 15   |
| 10 | -20 | -10  |

We can also multiply numbers and have commentary columns:

[//]: # (decisionTable Calculator)

| a  | b   | # comment  | mul? |
|----|-----|------------|------|
| 1  | 2   | This is a  | 2    |
| 2  | 2   | Commentary | 4    |
| 10 | 5   | It should  | 50   |
| 10 | -20 | be ignored | -200 |


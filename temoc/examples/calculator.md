### Calculator example

This is the example of an acceptance test for a calculator.

So, in this calculator we can sum values like this:

[//]: # "import Calculator.Fixtures"
[//]: # "decisionTable CalculatorFixture"

| a   | b   | sum? |
| --- | --- | ---- |
| 1   | 2   | 3    |
| 2   | 2   | 4    |
| 10  | 5   | 15   |
| 10  | -20 | -10  |

We can also multiply numbers and have commentary columns:

[//]: # "decisionTable CalculatorFixture"

| a   | b   | # comment  | mul? |
| --- | --- | ---------- | ---- |
| 1   | 2   | This is a  | 2    |
| 2   | 2   | Commentary | 4    |
| 10  | 5   | It should  | 50   |
| 10  | -20 | be ignored | -200 |

We can also use a single method instead of multiple setter and getters:

[//]: # "decisionTable CalculatorFixture#log"

| a   | b   | a log b? | b log a? |
| --- | --- | -------- | -------- |
| 1   | 2   | 0.00     | inf      |
| 2   | 2   | 1.00     | 1.00     |
| 20  | 10  | 1.30     | 0.77     |
| 1   | 10  | 0.00     | inf      |

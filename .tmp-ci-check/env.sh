#!/bin/bash
node -e "console.log(JSON.stringify(process.env.GITHUB_REF_NAME))"
